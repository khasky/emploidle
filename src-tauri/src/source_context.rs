#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Document,
    Notion,
    Jira,
    Repository,
    Mail,
    Calendar,
    Chat,
    Ide,
    Browser,
    Unknown,
}

fn host_matches(host: &str, base: &str) -> bool {
    host == base
        || host
            .strip_suffix(base)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

impl SourceKind {
    pub fn can_activate(self) -> bool {
        matches!(
            self,
            Self::Document
                | Self::Notion
                | Self::Jira
                | Self::Repository
                | Self::Calendar
                | Self::Ide
                | Self::Browser
        )
    }

    pub fn can_scroll(self) -> bool {
        self.can_activate()
    }

    pub fn can_browse(self) -> bool {
        matches!(
            self,
            Self::Notion | Self::Jira | Self::Repository | Self::Calendar
        )
    }

    pub fn browser_address(value: &str) -> Option<tauri::Url> {
        if value.len() > 512 {
            return None;
        }
        let value = if value.contains("://") {
            value.to_owned()
        } else {
            format!("https://{value}")
        };
        let url = tauri::Url::parse(&value).ok()?;
        (url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
            .then_some(url)
    }

    pub fn allows_link(self, current: &tauri::Url, candidate: &tauri::Url, visited: bool) -> bool {
        if !visited
            || !self.can_browse()
            || candidate.scheme() != "https"
            || candidate.origin() != current.origin()
            || candidate.query().is_some()
            || !candidate.username().is_empty()
            || candidate.password().is_some()
        {
            return false;
        }
        self.allows_page(candidate)
    }

    pub fn allows_page(self, address: &tauri::Url) -> bool {
        if address.scheme() != "https"
            || address.query().is_some()
            || !address.username().is_empty()
            || address.password().is_some()
        {
            return false;
        }
        let parts: Vec<_> = address
            .path()
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        match self {
            Self::Repository => {
                (parts.len() == 2
                    && !matches!(
                        parts[0],
                        "notifications"
                            | "settings"
                            | "users"
                            | "groups"
                            | "dashboard"
                            | "search"
                            | "admin"
                    ))
                    || (parts.len() >= 4 && matches!(parts[2], "blob" | "tree"))
                    || (parts.len() >= 5
                        && parts
                            .windows(2)
                            .any(|pair| pair[0] == "-" && matches!(pair[1], "blob" | "tree")))
            }
            Self::Jira if parts.len() == 2 && parts[0] == "browse" => {
                parts[1].rsplit_once('-').is_some_and(|(project, issue)| {
                    !project.is_empty()
                        && project
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                        && !issue.is_empty()
                        && issue.bytes().all(|b| b.is_ascii_digit())
                })
            }
            Self::Notion
                if parts.len() <= 2
                    && !parts
                        .iter()
                        .any(|part| matches!(*part, "api" | "settings" | "login")) =>
            {
                parts.last().is_some_and(|part| {
                    let id = part.rsplit('-').next().unwrap_or(part);
                    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            }
            Self::Calendar => parts.first() == Some(&"calendar"),
            _ => false,
        }
    }

    pub fn detect(domain: Option<&str>, application: &str) -> Self {
        if let Some(host) = domain {
            let host = host.to_ascii_lowercase();
            for (base, kind) in [
                ("mail.google.com", Self::Mail),
                ("outlook.live.com", Self::Mail),
                ("outlook.office.com", Self::Mail),
                ("outlook.office365.com", Self::Mail),
                ("calendar.google.com", Self::Calendar),
                ("slack.com", Self::Chat),
                ("discord.com", Self::Chat),
                ("teams.microsoft.com", Self::Chat),
                ("teams.live.com", Self::Chat),
                ("chat.google.com", Self::Chat),
                ("web.whatsapp.com", Self::Chat),
                ("web.telegram.org", Self::Chat),
                ("notion.so", Self::Notion),
                ("notion.site", Self::Notion),
                ("atlassian.net", Self::Jira),
                ("github.com", Self::Repository),
                ("gitlab.com", Self::Repository),
            ] {
                if host_matches(&host, base) {
                    return kind;
                }
            }
        }
        let application = application.to_ascii_lowercase();
        match application.trim_end_matches(".exe") {
            "slack" | "discord" => Self::Chat,
            "outlook" | "thunderbird" | "mail" => Self::Mail,
            "code" | "cursor" | "windsurf" | "idea64" | "pycharm64" | "webstorm64" | "rider64"
            | "clion64" | "goland64" | "rustrover64" => Self::Ide,
            "notepad" | "winword" | "wordpad" | "excel" | "powerpnt" | "soffice" | "textedit"
            | "gedit" | "kate" | "evince" | "okular" | "sumatrapdf" | "acrord32" => Self::Document,
            _ if domain.is_some() && Self::is_browser(&application) => Self::Browser,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "Document",
            Self::Notion => "Notion document",
            Self::Jira => "Jira view",
            Self::Repository => "Repository view",
            Self::Mail => "Mail view",
            Self::Calendar => "Calendar view",
            Self::Chat => "Channel view",
            Self::Ide => "IDE view",
            Self::Browser => "Browser page",
            Self::Unknown => "Accessible view",
        }
    }

    pub fn is_browser(application: &str) -> bool {
        matches!(
            application.to_ascii_lowercase().trim_end_matches(".exe"),
            "chrome" | "chromium" | "msedge" | "brave" | "vivaldi" | "opera"
        )
    }

    pub fn allows_reading(address: &str) -> bool {
        let Ok(url) = tauri::Url::parse(address) else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return false;
        }
        match Self::detect(url.host_str(), "chrome.exe") {
            Self::Chat => false,
            Self::Mail => {
                let gmail = url.host_str() == Some("mail.google.com")
                    && matches!(
                        url.fragment(),
                        Some("inbox" | "sent" | "starred" | "all" | "drafts")
                    );
                let outlook = url.host_str().is_some_and(|host| {
                    [
                        "outlook.live.com",
                        "outlook.office.com",
                        "outlook.office365.com",
                    ]
                    .iter()
                    .any(|base| host_matches(host, base))
                }) && matches!(
                    url.path().trim_end_matches('/'),
                    "/mail/inbox" | "/mail/0/inbox" | "/mail/sentitems" | "/mail/0/sentitems"
                );
                url.query().is_none() && (gmail || outlook)
            }
            _ => {
                let communication_host = url.host_str().is_some_and(|host| {
                    host.split('.')
                        .any(|part| matches!(part, "chat" | "mail" | "webmail" | "messenger"))
                });
                !communication_host
                    && !url
                        .path()
                        .split('/')
                        .chain(url.fragment().unwrap_or_default().split('/'))
                        .any(|part| {
                            matches!(
                                part.to_ascii_lowercase().as_str(),
                                "inbox"
                                    | "notifications"
                                    | "chat"
                                    | "messages"
                                    | "unread"
                                    | "room"
                                    | "rooms"
                                    | "channel"
                                    | "channels"
                                    | "conversation"
                                    | "conversations"
                                    | "dm"
                            )
                        })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_states_prevent_activating_or_scrolling_communication_views() {
        for kind in [SourceKind::Mail, SourceKind::Chat, SourceKind::Unknown] {
            assert!(!kind.can_activate());
            assert!(!kind.can_scroll());
            assert!(!kind.can_browse());
        }
        for kind in [
            SourceKind::Document,
            SourceKind::Ide,
            SourceKind::Notion,
            SourceKind::Jira,
            SourceKind::Repository,
            SourceKind::Calendar,
        ] {
            assert!(kind.can_activate());
            assert!(kind.can_scroll());
        }
    }

    #[test]
    fn browsing_reuses_https_and_only_visits_known_read_only_links() {
        let base = SourceKind::browser_address("github.com/team/project").unwrap();
        let file =
            tauri::Url::parse("https://github.com/team/project/blob/main/src/main.rs").unwrap();
        assert!(SourceKind::Repository.allows_link(&base, &file, true));
        assert!(!SourceKind::Repository.allows_link(&base, &file, false));
        for address in [
            "https://example.org/team/project/blob/main/file",
            "https://github.com/logout",
            "https://github.com/team/project/settings",
            "https://github.com/team/project/blob/main/file?action=delete",
        ] {
            assert!(!SourceKind::Repository.allows_link(
                &base,
                &tauri::Url::parse(address).unwrap(),
                true
            ));
        }
        for address in [
            "http://github.com/team/project",
            "javascript:alert(1)",
            "https://name:secret@github.com/team/project",
        ] {
            assert!(SourceKind::browser_address(address).is_none());
        }
        let gitlab =
            tauri::Url::parse("https://gitlab.com/team/project/-/blob/main/src/main.rs").unwrap();
        assert!(SourceKind::Repository.allows_link(&gitlab, &gitlab, true));
        for address in [
            "https://github.com/notifications",
            "https://github.com/team/project/issues/1",
            "https://workspace.notion.so/inbox",
            "https://team.atlassian.net/settings",
        ] {
            let address = tauri::Url::parse(address).unwrap();
            assert!(!SourceKind::detect(address.host_str(), "chrome.exe").allows_page(&address));
        }
        let jira = tauri::Url::parse("https://team.atlassian.net/browse/APP-10").unwrap();
        assert!(SourceKind::Jira.allows_link(&jira, &jira, true));
        assert!(!SourceKind::Mail.allows_link(&jira, &jira, true));
        assert!(!SourceKind::Chat.allows_link(&jira, &jira, true));
    }

    #[test]
    fn identifies_supported_services_without_accepting_lookalike_hosts() {
        for (domain, expected) in [
            ("mail.google.com", SourceKind::Mail),
            ("calendar.google.com", SourceKind::Calendar),
            ("app.slack.com", SourceKind::Chat),
            ("discord.com", SourceKind::Chat),
            ("workspace.notion.site", SourceKind::Notion),
            ("team.atlassian.net", SourceKind::Jira),
            ("github.com", SourceKind::Repository),
            ("gitlab.com", SourceKind::Repository),
        ] {
            assert_eq!(SourceKind::detect(Some(domain), "chrome.exe"), expected);
            assert_eq!(
                SourceKind::detect(Some(&format!("{domain}.example.org")), "chrome.exe"),
                SourceKind::Browser
            );
        }
        assert_eq!(
            SourceKind::detect(Some("evilgithub.com"), "chrome.exe"),
            SourceKind::Browser
        );
        assert!(!SourceKind::Browser.can_browse());
        assert!(!SourceKind::Browser
            .allows_page(&tauri::Url::parse("https://evilgithub.com/team/project").unwrap()));
    }

    #[test]
    fn ordinary_browser_pages_remain_available_for_reading() {
        assert_eq!(
            SourceKind::detect(Some("example.org"), "chrome.exe"),
            SourceKind::Browser
        );
        assert!(SourceKind::Browser.can_activate());
        assert!(SourceKind::Browser.can_scroll());
        for address in [
            "https://example.org/article",
            "http://localhost:3000/docs",
            "https://github.com/team/project?tab=readme",
        ] {
            assert!(SourceKind::allows_reading(address), "{address}");
        }
    }

    #[test]
    fn communication_lists_can_be_read_without_opening_a_conversation() {
        assert!(SourceKind::allows_reading(
            "https://mail.google.com/mail/u/0/#inbox"
        ));
        assert!(SourceKind::allows_reading(
            "https://outlook.office.com/mail/inbox"
        ));
        for address in [
            "https://mail.google.com/mail/u/0/#inbox/message-id",
            "https://mail.google.com/mail/u/0/?compose=new#inbox",
            "https://outlook.office.com/mail/inbox/id/message-id",
            "https://app.slack.com/client/workspace/channel",
            "https://discord.com/channels/server/channel",
            "https://example.org/notifications",
            "https://example.org/chat",
            "https://example.org/#messages",
            "https://chat.example.org/room/general",
            "https://example.org/room/general",
            "https://messenger.example.org/",
            "javascript:alert(1)",
            "https://name:password@example.org/article",
        ] {
            assert!(!SourceKind::allows_reading(address), "{address}");
        }
    }
}
