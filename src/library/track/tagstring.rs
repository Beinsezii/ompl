use super::Tags;

fn parse_internal(internal: &str, tags: &Tags) -> String {
    let mut result = internal.to_string();
    if internal.starts_with('<')
        && internal.ends_with('>')
        && internal.chars().nth(internal.len() - 2) != Some('\\')
    {
        result.remove(0);
        result.pop();
        let mut cond = None;
        let mut iter = result.char_indices();
        loop {
            let (n, c) = match iter.next() {
                Some(i) => i,
                None => break,
            };
            match c {
                '\\' => drop(iter.next()),
                '|' => cond = Some(n),
                _ => (),
            }
        }
        if let Some(mut cond) = cond {
            let mut inv = false;
            if result.starts_with('!') {
                result.remove(0);
                cond -= 1;
                inv = true;
            }
            match tags.get(&result[0..cond].to_ascii_lowercase()).is_some() ^ inv {
                true => result.replace_range(0..=cond, ""),
                false => result = String::new(),
            }
        } else {
            match tags.get(&result.to_ascii_lowercase()) {
                Some(tag) => result = tag.to_string(),
                None => result = String::from("???"),
            }
        }
    }
    result
}

pub fn parse<T: Into<String>>(tagstring: T, tags: &Tags) -> String {
    let mut result = tagstring.into();
    let mut literal = true;
    loop {
        let mut start = None;
        let mut end = None;
        let mut iter = result.char_indices();
        loop {
            let (n, c) = match iter.next() {
                Some(i) => i,
                None => break,
            };
            match c {
                '\\' => drop(iter.next()),
                '<' => start = Some(n),
                '>' => {
                    if let Some(s) = start {
                        // find first end after last start
                        if s < n {
                            end = Some(n);
                            literal = false;
                            break;
                        }
                    }
                }
                _ => (),
            }
        }
        if let (Some(s), Some(e)) = (start, end) {
            result.replace_range(s..=e, &parse_internal(&result[s..=e], &tags))
        } else {
            break;
        }
    }

    // clean up escapes
    let mut iter = result.chars();
    let mut buff = String::with_capacity(result.len());
    loop {
        match iter.next() {
            Some('\\') => {
                if let Some(c) = iter.next() {
                    buff.push(c)
                }
            }
            Some(c) => buff.push(c),
            None => break,
        }
    }
    result = buff;
    result.shrink_to_fit();

    if literal {
        tags.get(&result.to_ascii_lowercase())
            .unwrap_or(&"???".to_string())
            .to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use super::Tags;
    lazy_static::lazy_static! {
    static ref TAGS: Tags = Tags::from([
        (String::from("tit1"), String::from("TheTitle")),
        (String::from("title"), String::from("TheTitle")),
        (String::from("talb"), String::from("TheAlbum")),
        (String::from("album"), String::from("TheAlbum")),
        (String::from("tcon"), String::from("TheGenre")),
        (String::from("genre"), String::from("TheGenre")),
    ]);
    }

    #[test]
    fn control() {
        assert_eq!(parse("Hello, World!", &TAGS), "???".to_string());
    }

    #[test]
    fn control_unequal() {
        assert_eq!(parse("Hello>, Worl<d!", &TAGS), "???".to_string());
    }

    #[test]
    fn control_unequal2() {
        assert_eq!(parse(">Hello, World!<", &TAGS), "???".to_string());
    }

    #[test]
    fn sub() {
        assert_eq!(parse("<title>", &TAGS), "TheTitle".to_string());
    }

    #[test]
    fn sub_case() {
        assert_eq!(parse("<TiTlE>", &TAGS), "TheTitle".to_string());
    }

    #[test]
    fn sub_literal() {
        assert_eq!(parse("TiT1", &TAGS), "TheTitle".to_string());
    }

    #[test]
    fn sub_before() {
        assert_eq!(
            parse("<title> is the title!", &TAGS),
            "TheTitle is the title!".to_string()
        );
    }

    #[test]
    fn sub_after() {
        assert_eq!(
            parse("The title is <title>", &TAGS),
            "The title is TheTitle".to_string()
        );
    }

    #[test]
    fn sub_inline() {
        assert_eq!(
            parse("This title: <title> is rad!", &TAGS),
            "This title: TheTitle is rad!".to_string()
        );
    }

    #[test]
    fn sub_multi() {
        assert_eq!(
            parse("Title: <title>, Album: <album>!", &TAGS),
            "Title: TheTitle, Album: TheAlbum!".to_string()
        );
    }

    #[test]
    fn escape() {
        assert_eq!(parse(r#"\<title>"#, &TAGS), "???".to_string());
    }

    #[test]
    fn mixed() {
        assert_eq!(
            parse(
                r#"Title: \<title\>, Album: <<album>>>, Genre: <genre>, done!"#,
                &TAGS
            ),
            r#"Title: <title>, Album: ???>, Genre: TheGenre, done!"#.to_string()
        );
    }

    #[test]
    fn condition_true() {
        assert_eq!(
            parse("Tag?<title| Title: <title>!>", &TAGS),
            "Tag? Title: TheTitle!".to_string()
        );
    }

    #[test]
    fn condition_false() {
        assert_eq!(
            parse("Tag?<badtag| Badtag: <badtag>!>", &TAGS),
            "Tag?".to_string()
        );
    }

    #[test]
    fn condition_invert_true() {
        assert_eq!(
            parse("Tag?<!title| Title: <title>!>", &TAGS),
            "Tag?".to_string()
        );
    }

    #[test]
    fn condition_invert_false() {
        assert_eq!(
            parse("Tag?<!badtag| Badtag: <badtag>!>", &TAGS),
            "Tag? Badtag: ???!".to_string()
        );
    }

    #[test]
    fn condition_mixed() {
        assert_eq!(
            parse(
                r#"<mood|This is a very <mood> song~><!mood|\<title\>: <title><TALB| is part of <TALB>>>"#,
                &TAGS
            ),
            "<title>: TheTitle is part of TheAlbum".to_string()
        );
    }
}
