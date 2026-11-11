//! The errors a link call reports. Each has a stable code; a message says what to do.

use std::fmt;

/// What went wrong, as a stable code; see [`DeepLinksError::code`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The link is not an absolute URL, or has whitespace or control characters.
    InvalidUrl,
    /// The link is longer than `nsh_core::url_scheme::MAX_URL_LEN` bytes.
    UrlTooLarge,
    /// The link's scheme is neither one the app claims nor `http(s)` for verified web links.
    DisallowedScheme,
    /// The `http(s)` link's host is not one the route table's prefixes allow.
    HostNotAllowed,
    /// The link's path matches no pattern in the route table.
    NoRoute,
    /// A link parameter failed the schema its route declares for it.
    InvalidParameter,
    /// An argument a call takes is empty or of the wrong shape.
    InvalidArgument,
    /// The platform cannot deliver verified web links here (desktop, web).
    UnsupportedPlatform,
    /// Another wait is already running. One link is waited for at a time.
    Busy,
    /// The wait was cancelled by `stop`, or its future dropped.
    Cancelled,
    /// The platform gave no answer before its deadline.
    Timeout,
    /// An association file is malformed, or names the wrong app.
    AssociationInvalid,
    /// The platform failed; `detail` holds its own message where it gives one.
    Backend,
}

/// A failed link call or route match.
///
/// `Display` names the code's message, and the detail when there is one. The detail never
/// holds link text or other data that came from outside the app.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .joined())]
pub struct DeepLinksError {
    kind: ErrorKind,
    detail: Option<String>,
}

impl DeepLinksError {
    /// An error of `kind`, with no detail.
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind, detail: None }
    }

    /// The same error, with a sentence saying what specifically went wrong.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// What went wrong.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The detail, when there is one.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The stable code, such as `ERR_DEEP_LINKS_CANCELLED`. Apps match on this.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    fn joined(&self) -> String {
        match self.detail.as_deref() {
            Some(detail) => format!("{} {detail}", self.kind.message()),
            None => self.kind.message().to_owned(),
        }
    }
}

impl ErrorKind {
    /// The stable code of this kind.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::InvalidUrl => "ERR_DEEP_LINKS_INVALID_URL",
            ErrorKind::UrlTooLarge => "ERR_DEEP_LINKS_URL_TOO_LARGE",
            ErrorKind::DisallowedScheme => "ERR_DEEP_LINKS_DISALLOWED_SCHEME",
            ErrorKind::HostNotAllowed => "ERR_DEEP_LINKS_HOST_NOT_ALLOWED",
            ErrorKind::NoRoute => "ERR_DEEP_LINKS_NO_ROUTE",
            ErrorKind::InvalidParameter => "ERR_DEEP_LINKS_INVALID_PARAMETER",
            ErrorKind::InvalidArgument => "ERR_DEEP_LINKS_INVALID_ARGUMENT",
            ErrorKind::UnsupportedPlatform => "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM",
            ErrorKind::Busy => "ERR_DEEP_LINKS_BUSY",
            ErrorKind::Cancelled => "ERR_DEEP_LINKS_CANCELLED",
            ErrorKind::Timeout => "ERR_DEEP_LINKS_TIMEOUT",
            ErrorKind::AssociationInvalid => "ERR_DEEP_LINKS_ASSOCIATION_INVALID",
            ErrorKind::Backend => "ERR_DEEP_LINKS_BACKEND",
        }
    }

    /// What to do about it, in one sentence.
    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::InvalidUrl => "The link is not an absolute URL a link may be.",
            ErrorKind::UrlTooLarge => {
                "The link is too long. Links have at most 16 KiB of text."
            }
            ErrorKind::DisallowedScheme => {
                "The link's scheme is not the app's own and not a verified web link."
            }
            ErrorKind::HostNotAllowed => {
                "The link's host is not one the app's prefixes allow."
            }
            ErrorKind::NoRoute => "The link's path matches no route in the table.",
            ErrorKind::InvalidParameter => {
                "A link parameter is not of the type its route declares."
            }
            ErrorKind::InvalidArgument => "An argument is empty or of the wrong shape.",
            ErrorKind::UnsupportedPlatform => {
                "Verified web links are not available on this platform."
            }
            ErrorKind::Busy => {
                "Another link wait is already running. Wait for it to finish."
            }
            ErrorKind::Cancelled => "The link wait was cancelled.",
            ErrorKind::Timeout => "The platform gave no link before its deadline.",
            ErrorKind::AssociationInvalid => {
                "The association file is malformed, or names the wrong app."
            }
            ErrorKind::Backend => "The platform's link service failed.",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [(ErrorKind, &str); 13] = [
        (ErrorKind::InvalidUrl, "ERR_DEEP_LINKS_INVALID_URL"),
        (ErrorKind::UrlTooLarge, "ERR_DEEP_LINKS_URL_TOO_LARGE"),
        (
            ErrorKind::DisallowedScheme,
            "ERR_DEEP_LINKS_DISALLOWED_SCHEME",
        ),
        (ErrorKind::HostNotAllowed, "ERR_DEEP_LINKS_HOST_NOT_ALLOWED"),
        (ErrorKind::NoRoute, "ERR_DEEP_LINKS_NO_ROUTE"),
        (
            ErrorKind::InvalidParameter,
            "ERR_DEEP_LINKS_INVALID_PARAMETER",
        ),
        (
            ErrorKind::InvalidArgument,
            "ERR_DEEP_LINKS_INVALID_ARGUMENT",
        ),
        (
            ErrorKind::UnsupportedPlatform,
            "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM",
        ),
        (ErrorKind::Busy, "ERR_DEEP_LINKS_BUSY"),
        (ErrorKind::Cancelled, "ERR_DEEP_LINKS_CANCELLED"),
        (ErrorKind::Timeout, "ERR_DEEP_LINKS_TIMEOUT"),
        (
            ErrorKind::AssociationInvalid,
            "ERR_DEEP_LINKS_ASSOCIATION_INVALID",
        ),
        (ErrorKind::Backend, "ERR_DEEP_LINKS_BACKEND"),
    ];

    #[test]
    fn every_kind_has_its_stable_code() {
        for (kind, code) in ALL {
            assert_eq!(kind.code(), code);
            assert_eq!(kind.to_string(), code);
        }
    }

    #[test]
    fn a_kind_is_named_by_its_code_and_never_empty() {
        for (kind, _) in ALL {
            assert!(!kind.message().is_empty(), "{}", kind.code());
        }
    }

    #[test]
    fn the_detail_joins_the_message_without_losing_the_code() {
        let plain = DeepLinksError::new(ErrorKind::NoRoute);
        assert_eq!(plain.code(), "ERR_DEEP_LINKS_NO_ROUTE");
        assert_eq!(plain.detail(), None);
        let with = plain.with_detail("no prefix matches the URL");
        assert_eq!(with.kind(), ErrorKind::NoRoute);
        assert_eq!(with.detail(), Some("no prefix matches the URL"));
        assert!(with.to_string().contains("no prefix matches the URL"));
        assert!(with.to_string().contains("route"));
    }
}
