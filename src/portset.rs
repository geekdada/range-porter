use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortSetError {
    message: String,
}

impl PortSetError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for PortSetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PortSetError {}

pub fn parse_portset(input: &str) -> Result<Vec<u16>, PortSetError> {
    if input.trim().is_empty() {
        return Err(PortSetError::new("listen port expression cannot be empty"));
    }

    let mut ports = BTreeSet::new();

    for segment in input.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(PortSetError::new(
                "listen port expression contains an empty segment",
            ));
        }

        if let Some((start, end)) = segment.split_once('-') {
            let start = parse_port(start)?;
            let end = parse_port(end)?;

            if start > end {
                return Err(PortSetError::new(format!(
                    "invalid port range {segment}: start must be <= end"
                )));
            }

            ports.extend(start..=end);
            continue;
        }

        ports.insert(parse_port(segment)?);
    }

    Ok(ports.into_iter().collect())
}

fn parse_port(segment: &str) -> Result<u16, PortSetError> {
    let port: u16 = segment
        .trim()
        .parse()
        .map_err(|_| PortSetError::new(format!("invalid port value: {segment}")))?;

    if port == 0 {
        return Err(PortSetError::new("port 0 is not a valid listen port"));
    }

    Ok(port)
}
