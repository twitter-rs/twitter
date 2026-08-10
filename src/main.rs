use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use twitter::{Store, Target, Twitter};

#[derive(Parser)]
#[command(
    name = "twitter",
    version,
    about = "Offline viewer and media downloader for X (Twitter)",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    /// Cookies file (Netscape format) with auth_token and ct0 entries
    #[arg(short = 'c', long, global = true, default_value = "cookies.txt")]
    cookies: PathBuf,
    /// Output directory for downloaded posts, media and the offline viewer
    #[arg(short = 'o', long, global = true, default_value = "twitter_data")]
    out: PathBuf,
    /// Suppress progress output
    #[arg(short = 'q', long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download posts and media from accounts (@user) or post URLs
    Download {
        /// Account names (@user), post URLs (https://x.com/user/status/123) or post ids
        #[arg(required = true, value_name = "TARGET")]
        targets: Vec<String>,
        /// Maximum number of posts per account
        #[arg(short = 'n', long, value_name = "N")]
        max: Option<usize>,
        /// Only save post metadata, skip media files
        #[arg(long)]
        no_media: bool,
        /// Skip retweets when downloading an account
        #[arg(long)]
        no_retweets: bool,
    },
    /// Print post or account info as JSON
    Info {
        /// Account name (@user), post URL or post id
        target: String,
    },
    /// Build the offline HTML viewer (and optionally serve it)
    View {
        /// Serve the viewer over HTTP, optionally on a specific port
        #[arg(long, value_name = "PORT", num_args = 0..=1, default_missing_value = "8080")]
        serve: Option<u16>,
        /// Open the viewer in your default browser
        #[arg(long)]
        open: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Commands::Download {
            targets,
            max,
            no_media,
            no_retweets,
        } => run_download(&cli, targets, *max, *no_media, *no_retweets),
        Commands::Info { target } => run_info(&cli, target),
        Commands::View { serve, open } => run_view(&cli, *serve, *open),
    }
}

fn client(cli: &Cli) -> Result<Twitter> {
    if cli.cookies.exists() {
        Twitter::from_cookies_file(&cli.cookies)
            .with_context(|| format!("failed to load cookies from {}", cli.cookies.display()))
    } else {
        if !cli.quiet {
            eprintln!(
                "note: no cookies file at {} — continuing unauthenticated (public posts only; accounts need cookies)",
                cli.cookies.display()
            );
        }
        Ok(Twitter::new()?)
    }
}

fn run_download(
    cli: &Cli,
    targets: &[String],
    max: Option<usize>,
    no_media: bool,
    no_retweets: bool,
) -> Result<()> {
    let tw = client(cli)?;
    let mut store = Store::load(&cli.out)?;
    let mut new_posts = 0usize;
    let mut media_files = 0usize;

    for target_str in targets {
        let target =
            Target::parse(target_str).with_context(|| format!("invalid target: {target_str}"))?;
        match target {
            Target::User(screen_name) => {
                let user = tw.user(&screen_name)?;
                if !cli.quiet {
                    println!("@{:?}: fetching timeline", user.screen_name);
                }
                let tweets = tw.timeline(&user, max)?;
                let tweets: Vec<_> = tweets
                    .into_iter()
                    .filter(|t| !(no_retweets && t.is_retweet))
                    .collect();
                if !cli.quiet {
                    println!("  {} posts fetched", tweets.len());
                }
                for t in &tweets {
                    if store.add(t.clone()) {
                        new_posts += 1;
                    }
                    if !no_media {
                        let files = tw.download_media(t, &cli.out)?;
                        media_files += files.len();
                        if !cli.quiet {
                            println!("  {}: {} media files", t.id, files.len());
                        }
                    }
                }
            }
            Target::Tweet(id) => {
                let t = tw.tweet(&id)?;
                if store.add(t.clone()) {
                    new_posts += 1;
                }
                if !no_media {
                    let files = tw.download_media(&t, &cli.out)?;
                    media_files += files.len();
                }
                if !cli.quiet {
                    println!("  {}: {} media files", t.id, t.media.len());
                }
            }
        }
    }

    store.save(&cli.out)?;
    if !cli.quiet {
        println!(
            "done: {} new posts, {} media files in {} (viewer: {})",
            new_posts,
            media_files,
            cli.out.display(),
            cli.out.join("index.html").display()
        );
    }
    Ok(())
}

fn run_info(cli: &Cli, target: &str) -> Result<()> {
    let tw = client(cli)?;
    match Target::parse(target)? {
        Target::User(screen_name) => {
            let user = tw.user(&screen_name)?;
            println!("{}", serde_json::to_string_pretty(&user)?);
        }
        Target::Tweet(id) => {
            let t = tw.tweet(&id)?;
            println!("{}", serde_json::to_string_pretty(&t)?);
        }
    }
    Ok(())
}

fn run_view(cli: &Cli, serve: Option<u16>, open: bool) -> Result<()> {
    let store = Store::load(&cli.out)?;
    if store.is_empty() {
        anyhow::bail!(
            "no data in {} — run `twitter download <target>` first",
            cli.out.display()
        );
    }
    store.save(&cli.out)?;
    if let Some(port) = serve {
        let url = format!("http://127.0.0.1:{port}/");
        println!("viewer: {url}");
        if open {
            open_url(&url);
        }
        twitter::viewer::serve(&cli.out, port)?;
    } else {
        let file = cli.out.join("index.html");
        if open {
            open_url(&format!("file://{}", file.display()));
        }
        println!("viewer written to {}", file.display());
    }
    Ok(())
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/c", "start", "", url]).spawn();
}
