use crate::{runtime::error::local, Result};
use std::num::NonZeroUsize;

pub(super) enum Query {
    Latest,
    List,
    Index(NonZeroUsize),
}
pub(super) fn parse(message: &str) -> Result<Query> {
    let args = message.split_whitespace().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(Query::Latest),
        ["list"] => Ok(Query::List),
        [index] => index
            .parse::<NonZeroUsize>()
            .map(Query::Index)
            .map_err(|_| local("Use /output, /output list, or /output <positive number>.").into()),
        _ => Err(local("Use /output, /output list, or /output <positive number>.").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_arguments_require_a_single_positive_index_or_list() {
        assert!(matches!(parse("/output").unwrap(), Query::Latest));
        assert!(matches!(parse("/output list").unwrap(), Query::List));
        assert!(matches!(parse("/output 2").unwrap(), Query::Index(n) if n.get()==2));
        for text in [
            "/output 0",
            "/output -1",
            "/output 2 extra",
            "/output invalid",
            "/output 999999999999999999999999999",
        ] {
            assert!(parse(text).is_err());
        }
    }
}
