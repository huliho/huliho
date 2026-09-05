// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

//! The Thunderbird autoconfig document, `config-v1.1.xml`: the IMAP and
//! SMTP servers it names over TLS.

use serde::Deserialize;

use super::address::{Address, named_host};
use crate::accounts::{Endpoint, TlsMode};

/// Which part of the address the servers take as username.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Username {
    Address,
    LocalPart,
}

impl Username {
    pub(super) fn of(self, address: &Address) -> String {
        match self {
            Self::Address => address.to_string(),
            Self::LocalPart => address.local().to_owned(),
        }
    }
}

/// The servers a document offers over TLS.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Servers {
    pub imap: Endpoint,
    pub smtp: Endpoint,
    pub username: Username,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientConfig {
    email_provider: EmailProvider,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmailProvider {
    #[serde(default)]
    incoming_server: Vec<Server>,
    #[serde(default)]
    outgoing_server: Vec<Server>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    #[serde(rename = "@type")]
    kind: String,
    hostname: Option<String>,
    port: Option<u16>,
    socket_type: Option<String>,
    username: Option<String>,
}

/// The first IMAP and the first SMTP server over TLS; `None` when the
/// document is not one or misses either. A `plain` socket type is
/// skipped.
pub(super) fn parse(document: &[u8]) -> Option<Servers> {
    let text = std::str::from_utf8(document).ok()?;
    let config: ClientConfig = quick_xml::de::from_str(text).ok()?;
    let provider = config.email_provider;
    let (imap, username) = provider
        .incoming_server
        .iter()
        .find_map(|server| Some((endpoint(server, "imap")?, username_of(server))))?;
    let smtp = provider
        .outgoing_server
        .iter()
        .find_map(|server| endpoint(server, "smtp"))?;
    Some(Servers {
        imap,
        smtp,
        username,
    })
}

fn endpoint(server: &Server, kind: &str) -> Option<Endpoint> {
    if server.kind != kind {
        return None;
    }
    let tls = match server.socket_type.as_deref()? {
        "SSL" => TlsMode::Implicit,
        "STARTTLS" => TlsMode::Starttls,
        _ => return None,
    };
    Some(Endpoint {
        host: named_host(server.hostname.as_deref()?)?,
        port: server.port?,
        tls,
    })
}

/// `%EMAILLOCALPART%` asks for the local part; every other value, the
/// usual `%EMAILADDRESS%` included, takes the whole address.
fn username_of(server: &Server) -> Username {
    match server.username.as_deref() {
        Some("%EMAILLOCALPART%") => Username::LocalPart,
        _ => Username::Address,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GMAIL: &str = r#"<clientConfig version="1.1">
  <emailProvider id="googlemail.com">
    <domain>gmail.com</domain>
    <displayName>Google Mail</displayName>
    <incomingServer type="imap">
      <hostname>imap.gmail.com</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>OAuth2</authentication>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <incomingServer type="pop3">
      <hostname>pop.gmail.com</hostname>
      <port>995</port>
      <socketType>SSL</socketType>
      <pop3><leaveMessagesOnServer>true</leaveMessagesOnServer></pop3>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.gmail.com</hostname>
      <port>465</port>
      <socketType>SSL</socketType>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
    <documentation url="http://mail.google.com/support"><descr>d</descr></documentation>
  </emailProvider>
  <oAuth2><issuer>accounts.google.com</issuer></oAuth2>
</clientConfig>"#;

    /// One server element; `smtp` is outgoing, every other kind incoming.
    fn server(kind: &str, host: &str, port: &str, socket: &str) -> String {
        let tag = if kind == "smtp" {
            "outgoingServer"
        } else {
            "incomingServer"
        };
        format!(
            "<{tag} type=\"{kind}\"><hostname>{host}</hostname><port>{port}</port>\
             <socketType>{socket}</socketType><username>%EMAILADDRESS%</username></{tag}>"
        )
    }

    fn document(servers: &str) -> String {
        format!(
            "<clientConfig version=\"1.1\"><emailProvider id=\"x\">{servers}</emailProvider></clientConfig>"
        )
    }

    fn imap(host: &str, port: &str, socket: &str) -> String {
        server("imap", host, port, socket)
    }

    fn smtp(host: &str, port: &str, socket: &str) -> String {
        server("smtp", host, port, socket)
    }

    fn endpoint(host: &str, port: u16, tls: TlsMode) -> Endpoint {
        Endpoint {
            host: host.to_owned(),
            port,
            tls,
        }
    }

    #[test]
    fn the_ispdb_gmail_document_names_both_servers() {
        let servers = parse(GMAIL.as_bytes()).unwrap();
        assert_eq!(
            servers.imap,
            endpoint("imap.gmail.com", 993, TlsMode::Implicit)
        );
        assert_eq!(
            servers.smtp,
            endpoint("smtp.gmail.com", 465, TlsMode::Implicit)
        );
        assert_eq!(servers.username, Username::Address);
    }

    #[test]
    fn a_plain_entry_is_skipped_for_the_next_one() {
        let text = document(&format!(
            "{}{}{}",
            imap("plain.example.test", "143", "plain"),
            imap("imap.example.test", "143", "STARTTLS"),
            smtp("smtp.example.test", "587", "STARTTLS")
        ));
        let servers = parse(text.as_bytes()).unwrap();
        assert_eq!(
            servers.imap,
            endpoint("imap.example.test", 143, TlsMode::Starttls)
        );
    }

    #[test]
    fn a_document_with_only_plain_servers_is_no_hit() {
        let text = document(&format!(
            "{}{}",
            imap("imap.example.test", "143", "plain"),
            smtp("smtp.example.test", "25", "plain")
        ));
        assert_eq!(parse(text.as_bytes()), None);
    }

    #[test]
    fn a_document_without_an_outgoing_server_is_no_hit() {
        let text = document(&imap("imap.example.test", "993", "SSL"));
        assert_eq!(parse(text.as_bytes()), None);
    }

    #[test]
    fn the_local_part_placeholder_is_honored() {
        let text = document(&format!(
            "{}{}",
            imap("imap.example.test", "993", "SSL").replace("%EMAILADDRESS%", "%EMAILLOCALPART%"),
            smtp("smtp.example.test", "465", "SSL")
        ));
        let servers = parse(text.as_bytes()).unwrap();
        assert_eq!(servers.username, Username::LocalPart);
        let address = Address::parse("sanne@example.test").unwrap();
        assert_eq!(Username::LocalPart.of(&address), "sanne");
        assert_eq!(Username::Address.of(&address), "sanne@example.test");
    }

    #[test]
    fn an_address_literal_host_is_skipped() {
        let text = document(&format!(
            "{}{}",
            imap("127.0.0.1", "993", "SSL"),
            smtp("smtp.example.test", "465", "SSL")
        ));
        assert_eq!(parse(text.as_bytes()), None);
    }

    #[test]
    fn interleaved_servers_parse() {
        let text = document(&format!(
            "{}{}{}",
            server("pop3", "pop.example.test", "995", "SSL"),
            smtp("smtp.example.test", "465", "SSL"),
            imap("imap.example.test", "993", "SSL")
        ));
        let servers = parse(text.as_bytes()).unwrap();
        assert_eq!(
            servers.imap,
            endpoint("imap.example.test", 993, TlsMode::Implicit)
        );
    }

    #[test]
    fn something_that_is_not_a_document_is_no_hit() {
        for bytes in [b"<html></html>".as_slice(), b"", b"\xff\xfe"] {
            assert_eq!(parse(bytes), None);
        }
    }
}
