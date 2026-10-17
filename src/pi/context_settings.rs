use std::{fs, io};

pub(super) fn load() -> io::Result<serde_json::Value> {
    let path = crate::paths::home()?.join("config.json");
    match fs::read(path) {
        Ok(bytes) => parse(&bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => parse(b"{}"),
        Err(error) => Err(error),
    }
}

fn parse(bytes: &[u8]) -> io::Result<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if !value.is_object() {
        return Err(io::Error::other("config.json must contain an object"));
    }
    let window = value
        .get("codexContextWindow")
        .cloned()
        .unwrap_or(serde_json::json!(1_000_000));
    if !window.is_null()
        && !window
            .as_u64()
            .is_some_and(|n| (200_000..=1_050_000).contains(&n))
    {
        return Err(io::Error::other(
            "codexContextWindow must be null or an integer from 200000 to 1050000",
        ));
    }
    Ok(serde_json::json!({"codexContextWindow": window}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_context_configuration() {
        assert_eq!(parse(b"{}").unwrap()["codexContextWindow"], 1_000_000);
        assert!(parse(br#"{"codexContextWindow":null}"#).unwrap()["codexContextWindow"].is_null());
        assert_eq!(
            parse(br#"{"codexContextWindow":1050000,"other":true}"#).unwrap()["codexContextWindow"],
            1_050_000
        );
        for input in [
            "[]",
            "null",
            r#"{"codexContextWindow":true}"#,
            r#"{"codexContextWindow":199999}"#,
            r#"{"codexContextWindow":1050001}"#,
            r#"{"codexContextWindow":1000000.5}"#,
        ] {
            assert!(parse(input.as_bytes()).is_err(), "{input}");
        }
    }
}
