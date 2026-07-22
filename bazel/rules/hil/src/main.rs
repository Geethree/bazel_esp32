use std::{
    collections::BTreeMap,
    env,
    error::Error,
    path::PathBuf,
    process::{Command, ExitCode},
};

use runfiles::{Runfiles, rlocation};

const HOST_ENV: &str = "HIL_RUNFILES_HOST";
const ARTIFACTS_ENV: &str = "HIL_RUNFILES_ARTIFACTS";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("HIL launcher: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn Error>> {
    let runfiles = Runfiles::create()?;
    let host_key = env::var(HOST_ENV).map_err(|_| format!("missing {HOST_ENV}"))?;
    let host = resolve(&runfiles, &host_key)?;
    let artifact_keys = parse_artifacts(&env::var(ARTIFACTS_ENV).unwrap_or_default())?;
    let artifacts = artifact_keys
        .into_iter()
        .map(|(name, key)| Ok((name, resolve(&runfiles, &key)?)))
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let args = env::args()
        .skip(1)
        .map(|arg| substitute_artifacts(&arg, &artifacts))
        .collect::<Result<Vec<_>, _>>()?;

    let status = Command::new(host).args(args).status()?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

fn resolve(runfiles: &Runfiles, key: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = rlocation!(runfiles, key).ok_or_else(|| format!("runfile not found: {key}"))?;
    if !path.exists() {
        return Err(format!("runfile does not exist: {}", path.display()).into());
    }
    Ok(path)
}

fn parse_artifacts(value: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    value
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, key) = line
                .split_once('\t')
                .ok_or_else(|| format!("invalid artifact entry: {line:?}"))?;
            if name.is_empty() || key.is_empty() {
                return Err(format!("invalid artifact entry: {line:?}").into());
            }
            Ok((name.to_owned(), key.to_owned()))
        })
        .collect()
}

fn substitute_artifacts(
    arg: &str,
    artifacts: &BTreeMap<String, PathBuf>,
) -> Result<String, Box<dyn Error>> {
    let mut resolved = arg.to_owned();
    for (name, path) in artifacts {
        resolved = resolved.replace(&format!("{{{name}}}"), &path.to_string_lossy());
    }
    if resolved.contains(['{', '}']) {
        return Err(format!("unknown or malformed artifact placeholder in {arg:?}").into());
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_artifacts() {
        let artifacts = parse_artifacts("firmware\t_main/fw\nespflash\tespflash+/espflash\n")
            .expect("valid artifacts");

        assert_eq!(artifacts["firmware"], "_main/fw");
        assert_eq!(artifacts["espflash"], "espflash+/espflash");
    }

    #[test]
    fn substitutes_multiple_artifacts() {
        let artifacts = BTreeMap::from([
            ("first".to_owned(), PathBuf::from("/runfiles/first")),
            ("second".to_owned(), PathBuf::from("/runfiles/second")),
        ]);

        assert_eq!(
            substitute_artifacts("{first}:{second}", &artifacts).expect("known placeholders"),
            "/runfiles/first:/runfiles/second"
        );
    }

    #[test]
    fn rejects_unknown_placeholders() {
        assert!(substitute_artifacts("{missing}", &BTreeMap::new()).is_err());
    }
}
