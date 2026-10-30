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
    Unknown,
}

fn host_matches(host: &str, base: &str) -> bool {
    host == base
        || host
            .strip_suffix(base)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

impl SourceKind {
    pub fn detect(domain: Option<&str>, application: &str) -> Self {
        if let Some(host) = domain {
            let host = host.to_ascii_lowercase();
            for (base, kind) in [
                ("mail.google.com", Self::Mail),
                ("calendar.google.com", Self::Calendar),
                ("slack.com", Self::Chat),
                ("discord.com", Self::Chat),
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
            "notepad" | "textedit" | "gedit" | "kate" | "evince" | "okular" | "sumatrapdf"
            | "acrord32" => Self::Document,
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
            Self::Unknown => "Accessible view",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                SourceKind::Unknown
            );
        }
        assert_eq!(
            SourceKind::detect(Some("evilgithub.com"), "chrome.exe"),
            SourceKind::Unknown
        );
    }
}
