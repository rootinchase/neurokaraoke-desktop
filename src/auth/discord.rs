// Create a new module file: src/auth/discord.rs
use anyhow::{anyhow, Result};
use tokio::net::TcpListener;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use url::Url;

/// The client configuration parameters for the Neuro Karaoke Discord integration.
pub struct DiscordOAuthConfig {
    pub client_id: &'static str,
    pub redirect_uri: &'static str,
}

// Global reference config for the Neuro Karaoke official application profile
pub const NEURO_KARAOKE_DISCORD: DiscordOAuthConfig = DiscordOAuthConfig {
    client_id: "1544639254188007494", // Official Application ID
    redirect_uri: "http://localhost:14442/auth/callback",
};

/// Launches the browser and blocks asynchronously until the user signs into Discord.
pub async fn capture_discord_token(config: &DiscordOAuthConfig) -> Result<String> {
    // 1. Build the explicit authorization URL requesting user identity verification scopes
    let auth_url = format!(
        "https://discord.com/oauth2/authorize?client_id={}&response_type=token&redirect_uri={}&scope=identify",
        config.client_id,
        urlencoding::encode(config.redirect_uri)
    );

    // 2. Safely trigger the operating system's default browser handler
    // 2. Open browser window
    if let Err(e) = open::that(&auth_url) {
        return Err(anyhow!("Failed to open default web browser: {}", e));
    }

    // 3. Bind to the local tracking port
    let listener = TcpListener::bind("127.0.0.1:14442").await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).await?;

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let raw_path = parts[1];

        // ─── TRANSACTION PHASE 1: Capture the initial implicit fragment hit ───
        if !raw_path.contains("access_token=") {
            // Serve an inline JS script to extract client fragments and bounce them as standard queries
            let js_bouncer = "<html><script>
                if (window.location.hash) {
                    // Replace the invisible hash marker with an explicit parameter marker
                    const query = window.location.hash.replace('#', '?');
                    window.location.href = window.location.origin + window.location.pathname + query;
                } else {
                    document.body.innerHTML = '<h2>Auth Failed: Missing access token fragment.</h2>';
                }
            </script><body>Processing implicit grant tokens...</body></html>";

            let mut stream = reader.into_inner();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                js_bouncer.len(),
                js_bouncer
            );
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;
            continue; // Keep socket server loop waiting for the JS redirect hit below
        }

        // ─── TRANSACTION PHASE 2: Process the processed query bounce hit ───
        let url_path = format!("http://localhost{}", raw_path);
        let parsed_url = Url::parse(&url_path)?;

        let token_opt = parsed_url.query_pairs()
            .find(|(key, _)| key == "access_token")
            .map(|(_, val)| val.into_owned());

        if let Some(token) = token_opt {
            // Success response message frame display layout
            let success_html = "<html><body style='font-family:sans-serif; text-align:center; padding-top:40px; background:#0A0E1A; color:white;'>
                <h2 style='color:#00D9FF;'>Authenticated Successfully!</h2>
                <p>You can close this tab and return to your Karaoke desktop application safely.</p>
            </body></html>";

            let mut stream = reader.into_inner();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
                success_html.len(),
                success_html
            );
            stream.write_all(response.as_bytes()).await?;
            stream.flush().await?;

            return Ok(token); // Return raw token out to auth_service pipeline wrappers
        }
    }
}