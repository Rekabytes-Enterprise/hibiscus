//! Recognize narrowly scoped literal curl probes. Everything else remains an
//! ordinary shell command, whose success can represent a change of state.
use regex::Regex;
use std::{net::IpAddr, sync::LazyLock};

static WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:'[^'\n]*'|"[^"\n]*"|[A-Za-z0-9._/:?=%+@,\[\]-]+)"#).unwrap()
});

pub(super) fn is_probe(mut shell: &str) -> bool {
    if shell.len() > 4096 || shell.contains(['$', '`', '!']) {
        return false;
    }
    let mut words = Vec::new();
    let mut completed = false;
    loop {
        shell = shell.trim_start_matches([' ', '\t', '\r']);
        if shell.is_empty() {
            return if words.is_empty() {
                completed
            } else {
                curl(&words)
            };
        }
        if let Some(rest) = shell.strip_prefix("&&").or_else(|| shell.strip_prefix(';')) {
            if !curl(&words) || rest.trim().is_empty() {
                return false;
            }
            completed = true;
            words.clear();
            shell = rest;
            continue;
        }
        if let Some(rest) = shell.strip_prefix('\n') {
            if !words.is_empty() {
                if !curl(&words) {
                    return false;
                }
                completed = true;
            }
            words.clear();
            shell = rest;
            continue;
        }
        let Some(word) = WORD.find(shell).map(|word| word.as_str()) else {
            return false;
        };
        shell = &shell[word.len()..];
        if !shell.is_empty()
            && !shell.starts_with([' ', '\t', '\r', '\n', ';'])
            && !shell.starts_with("&&")
        {
            return false;
        }
        let literal = match word.as_bytes()[0] {
            b'\'' | b'"' => &word[1..word.len() - 1],
            _ => word,
        };
        // Only a literal backslash-n in a permitted -w format is interpreted
        // below. A final backslash can escape the shell's closing quote.
        if literal.ends_with('\\') {
            return false;
        }
        words.push(literal);
    }
}

fn curl(words: &[&str]) -> bool {
    let Some((&"curl", options)) = words.split_first() else {
        return false;
    };
    let mut options = options.iter().copied();
    let mut urls = 0;
    while let Some(word) = options.next() {
        let accepted = match word {
            "--silent" | "--show-error" | "--fail" | "--head" | "--disable" | "--globoff" => true,
            "-o" | "--output" => matches!(options.next(), Some("/dev/null")),
            "-X" | "--request" => matches!(options.next(), Some("GET" | "HEAD")),
            "-w" | "--write-out" => options.next().is_some_and(status_format),
            "-m" | "--max-time" | "--connect-timeout" => options
                .next()
                .is_some_and(|value| value.parse::<u32>().is_ok()),
            word if word.starts_with('-') => {
                word.len() > 1
                    && word[1..]
                        .bytes()
                        .all(|ch| matches!(ch, b's' | b'S' | b'f' | b'I' | b'q' | b'g'))
            }
            url => {
                urls += 1;
                literal_loopback(url)
            }
        };
        if !accepted {
            return false;
        }
    }
    urls > 0
}

fn status_format(format: &str) -> bool {
    let labels = format
        .replace("%{http_code}", "")
        .replace("%{response_code}", "")
        .replace(r"\n", "");
    !format.starts_with('@')
        && labels.bytes().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, b' ' | b'_' | b'-' | b':' | b'.' | b'/')
        })
}

fn literal_loopback(raw: &str) -> bool {
    if raw.contains(['\\', '{', '}']) || raw.chars().any(char::is_whitespace) {
        return false;
    }
    reqwest::Url::parse(raw).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::is_probe;

    #[test]
    fn accepts_literal_loopback_checks_only() {
        for valid in [
            r#"curl -fsS -o /dev/null -w 'root: %{http_code}\n' http://localhost:3000/ && curl -I 'http://[::1]:3000/health'"#,
            "curl -s http://127.0.0.1:3000/\ncurl --request GET http://localhost:3000/health",
        ] {
            assert!(is_probe(valid), "{valid}");
        }
        for other in [
            "",
            "curl http://localhost:3000/ > file",
            "curl -d x http://localhost:3000/",
            "curl --request POST http://localhost:3000/",
            "curl http://localhost:3000/; touch file",
            "curl http://localhost:3000/ &&",
            "curl -L http://localhost:3000/",
            "curl -o file http://localhost:3000/",
            "curl -w '%output{file}' http://localhost:3000/",
            "curl -w @format http://localhost:3000/",
            "curl http://remote.invalid/",
            "curl 'http://localhost:3000/$(touch file)'",
            "curl http://localhost@remote.invalid",
        ] {
            assert!(!is_probe(other), "{other}");
        }
    }
}
