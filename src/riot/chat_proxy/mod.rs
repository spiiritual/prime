use tokio::net::TcpListener;

mod certificate;
mod config;
mod presence;
mod relay;

pub use certificate::ChatProxyError;
pub use config::LOCALHOST_DOMAIN;
pub use presence::PresenceStatus;

pub fn client_config_url_arg(port: u16) -> String {
    format!("--client-config-url=\"http://127.0.0.1:{port}\"")
}

pub async fn bind_loopback_listener() -> Result<(TcpListener, u16), ChatProxyError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    Ok((listener, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_url_quotes_the_loopback_address() {
        assert_eq!(
            client_config_url_arg(1234),
            "--client-config-url=\"http://127.0.0.1:1234\"".to_string()
        );
    }
}
