use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use std::io::{self, BufRead};

use super::HostPath;

pub struct RandomNameGenerator {
    cache_dir: HostPath,
}

impl RandomNameGenerator {
    pub fn new(cache_dir: HostPath) -> Self {
        Self { cache_dir }
    }

    pub fn random_name<F>(&self, filter: F) -> Result<String>
    where
        F: Fn(&str) -> Result<bool>,
    {
        fn from_file<F>(file: std::fs::File, filter: F) -> Result<String>
        where
            F: Fn(&str) -> Result<bool>,
        {
            let mut rng = rand::thread_rng();
            let reader = io::BufReader::new(file);
            let lines = reader.lines().collect::<Result<Vec<String>, _>>()?;
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

        // 1. Prefer the EFF short word list. See https://www.eff.org/dice for
        // more info.
        let eff = || -> Result<String> {
            let eff_word_list = self.cache_dir.join("eff_short_wordlist_1.txt");
            let file = match std::fs::File::open(&eff_word_list.as_host_raw()) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    println!("Downloading EFF short wordlist");
                    let url = "https://www.eff.org/files/2016/09/08/eff_short_wordlist_1.txt";
                    let body = reqwest::blocking::get(url)?.text()?;
                    std::fs::create_dir_all(self.cache_dir.as_host_raw())?;
                    std::fs::write(&eff_word_list.as_host_raw(), body)?;
                    std::fs::File::open(&eff_word_list.as_host_raw())?
                }
                Err(e) => return Err(e.into()),
            };
            from_file(file, |w| Ok(w.len() < 10 && filter(w)?))
        };

        // 2. /usr/share/dict/words
        let dict = || -> Result<String> {
            let file = std::fs::File::open("/usr/share/dict/words")?;
            from_file(file, |w| Ok(w.len() < 6 && filter(w)?))
        };

        match eff() {
            Ok(word) => return Ok(word),
            Err(e) => {
                println!("Warning: failed to extract word from EFF list: {e}");
            }
        }
        match dict() {
            Ok(word) => return Ok(word),
            Err(e) => {
                println!("Warning: failed to extract word from /usr/share/dict/words: {e}");
            }
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
}
