use super::Tags;

fn parse_internal(internal: &str, tags: &Tags) -> String {
    let mut result = internal.to_string();
    if internal.starts_with('<') && internal.ends_with('>') {
        result.remove(0);
        result.pop();
        // let cond_invert = result.chars().nth(0) == Some('!');
        // if cond_invert {
        //     result.remove(0);
        // }
        // let cond = internal.find('|').map(||);
        if let Some(tag) = tags.get(&result) {
            result = tag.to_string();
        }
    }
    result
}

pub fn parse<T: Into<String>>(tagstring: T, tags: &Tags) -> String {
    let mut result = tagstring.into();
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

    result
}

#[cfg(test)]
mod tests {
    use super::parse;
    use super::Tags;
    lazy_static::lazy_static! {
    static ref TAGS: Tags = Tags::from([
        (String::from("TIT1"), String::from("TheTitle")),
        (String::from("Title"), String::from("TheTitle")),
        (String::from("tit1"), String::from("TheTitle")),
        (String::from("title"), String::from("TheTitle")),
        (String::from("TALB"), String::from("TheAlbum")),
        (String::from("Album"), String::from("TheAlbum")),
        (String::from("talb"), String::from("TheAlbum")),
        (String::from("album"), String::from("TheAlbum")),
        (String::from("TCON"), String::from("TheGenre")),
        (String::from("Genre"), String::from("TheGenre")),
        (String::from("tcon"), String::from("TheGenre")),
        (String::from("genre"), String::from("TheGenre")),
    ]);
    }

    #[test]
    fn control() {
        assert_eq!(parse("Hello, World!", &TAGS), "Hello, World!".to_string());
    }

    #[test]
    fn control_unequal() {
        assert_eq!(
            parse("Hello>, Worl<d!", &TAGS),
            "Hello>, Worl<d!".to_string()
        );
    }

    #[test]
    fn control_unequal2() {
        assert_eq!(
            parse(">Hello, World!<", &TAGS),
            ">Hello, World!<".to_string()
        );
    }

    #[test]
    fn sub() {
        assert_eq!(parse("<title>", &TAGS), "TheTitle".to_string());
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
        assert_eq!(parse(r#"\<title>"#, &TAGS), "<title>".to_string());
    }

    #[test]
    fn mixed() {
        assert_eq!(
            parse(
                r#"Title: \<title\>, Album: <<album>>>, Genre: <genre>, done!"#,
                &TAGS
            ),
            r#"Title: <title>, Album: TheAlbum>, Genre: TheGenre, done!"#.to_string()
        );
    }
}
