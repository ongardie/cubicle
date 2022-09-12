use rand::seq::SliceRandom;
use std::io::{self, BufRead};

use super::HostPath;
use crate::somehow::{somehow as anyhow, warn, Context, Result};

pub struct RandomNameGenerator {
    cache_dir: HostPath,
    eff_url: &'static str, // overridden for unit tests
}

impl RandomNameGenerator {
    pub fn new(cache_dir: HostPath) -> Self {
        Self {
            cache_dir,
            eff_url: "https://www.eff.org/files/2016/09/08/eff_short_wordlist_1.txt",
        }
    }

    pub fn random_name<F>(&self, filter: F) -> Result<String>
    where
        F: Fn(&str) -> Result<bool>,
    {
        // 1. Prefer the EFF short word list. See https://www.eff.org/dice for
        // more info.
        let eff = || -> Result<String> {
            let file = self.download_or_open_eff_list()?;
            from_reader(file, |w| Ok(w.len() < 10 && filter(w)?))
        };
        match eff().context("failed to extract word from EFF list") {
            Ok(word) => return Ok(word),
            Err(e) => warn(e),
        }

        // 2. /usr/share/dict/words
        let dict = || -> Result<String> {
            let file = std::fs::File::open("/usr/share/dict/words").enough_context()?;
            from_reader(file, |w| Ok(w.len() < 6 && filter(w)?))
        };
        match dict().context("failed to extract word from `/usr/share/dict/words`") {
            Ok(word) => return Ok(word),
            Err(e) => warn(e),
        }

        // 3. Random 6 letters
        let mut rng = rand::thread_rng();
        let alphabet = [
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
            'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
        ];
        for _ in 0..20 {
            let word = std::iter::repeat_with(|| alphabet.choose(&mut rng).unwrap())
                .take(6)
                .collect::<String>();
            if filter(&word)? {
                return Ok(word);
            }
        }

        // 4. Random 32 letters
        let word = std::iter::repeat_with(|| alphabet.choose(&mut rng).unwrap())
            .take(32)
            .collect::<String>();
        if filter(&word)? {
            return Ok(word);
        }

        // 5. Give up.
        Err(anyhow!(
            "Failed to generate suitable random word with any strategy"
        ))
    }

    fn download_or_open_eff_list(&self) -> Result<std::fs::File> {
        let eff_word_list = self.cache_dir.join("eff_short_wordlist_1.txt");
        let file = match std::fs::File::open(&eff_word_list.as_host_raw()) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                println!("Downloading EFF short wordlist");
                let body = reqwest::blocking::get(self.eff_url)
                    .and_then(|resp| resp.text())
                    .with_context(|| {
                        format!("error downloading word list from {:?}", self.eff_url)
                    })?;
                std::fs::create_dir_all(self.cache_dir.as_host_raw()).todo_context()?;
                std::fs::write(&eff_word_list.as_host_raw(), body).todo_context()?;
                std::fs::File::open(&eff_word_list.as_host_raw()).todo_context()?
            }
            Err(e) => return Err(e).todo_context(),
        };
        Ok(file)
    }
}

fn from_reader<R, F>(reader: R, filter: F) -> Result<String>
where
    R: std::io::Read,
    F: Fn(&str) -> Result<bool>,
{
    let mut rng = rand::thread_rng();
    let reader = io::BufReader::new(reader);
    let lines = reader
        .lines()
        .collect::<Result<Vec<String>, _>>()
        .todo_context()?;
    for _ in 0..200 {
        if let Some(line) = lines.choose(&mut rng) {
            for word in line.split_ascii_whitespace() {
                if word.chars().all(char::is_numeric) {
                    // probably diceware numbers
                    continue;
                }
                if filter(word)? {
                    return Ok(word.to_owned());
                }
            }
        }
    }
    Err(anyhow!("found no suitable word"))
}

#[cfg(test)]
mod tests {
    use super::HostPath;
    use insta::assert_snapshot;

    #[test]
    fn download_or_open_eff_list() {
        let tmpdir = tempfile::tempdir().unwrap();
        let tmpdir_path = HostPath::try_from(tmpdir.path().canonicalize().unwrap()).unwrap();
        let mut gen = super::RandomNameGenerator::new(tmpdir_path);
        gen.eff_url = "will://not work";
        let err = gen
            .download_or_open_eff_list()
            .unwrap_err()
            .debug_without_backtrace();
        assert_snapshot!(err, @r###"
        error downloading word list from "will://not work"

        Caused by:
            0: builder error: invalid domain character
            1: invalid domain character
        "###);
    }
}
