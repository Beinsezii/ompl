use super::Tags;

fn parse_recurse(tagstring: &str, tags: &Tags) -> String {
    let mut result = String::new();

    let mut starts = 0;
    let mut ends = 0;
    let mut open = 0;
    let mut close;
    let mut iter = tagstring.char_indices();
    loop {
        match iter.next() {
            Some((n, c)) => match c {
                '\\' => {
                    if let Some(c) = iter.next() {
                        // reason for the extra check is because if there's an escape in the condition it
                        // gets pushed twice, as the push happens both on the scan and on the recurse.
                        // Only pushing if start == 0 *seems* to work since the last recurse will always
                        // have start == 0 due to the brackets being melted away, BUT something about this
                        // is very sus and I feel like I'll need more tests to figure out what.
                        // TL;DR: the solution is *too* easy and I'm worried...
                        if starts == 0 {
                            result.push(c.1)
                        }
                    }
                }
                '<' if starts == 0 => {
                    starts += 1;
                    open = n
                }
                '<' => starts += 1,
                '>' if starts > 0 => {
                    ends += 1;
                    close = n;
                    if ends == starts {
                        let substring = &tagstring[open + 1..close];
                        (starts, ends) = (0, 0);
                        if let Some((n, sep)) = substring
                            .char_indices()
                            .enumerate()
                            .find_map(|(n, ci)| if ci.1 == '|' { Some((n, ci)) } else { None })
                        {
                            let invert = substring.starts_with('!');
                            if invert && n > 1 {
                                // should be safe to slice @ 1.. cause '!' is always 1 byte right?
                                if !tags.contains_key(&substring[1..sep.0].to_ascii_lowercase()) {
                                    result.push_str(&parse_recurse(&substring[sep.0 + 1..], tags))
                                }
                                continue;
                            } else if n > 0 {
                                if tags.contains_key(&substring[0..sep.0].to_ascii_lowercase()) {
                                    result.push_str(&parse_recurse(&substring[sep.0 + 1..], tags))
                                }
                                continue;
                            }
                        }
                        // if there's no valid conditional, dum check.
                        // means <<album>> will resolve to get("<album>")
                        // instead of get(get("album")) like the old system.
                        // probably for the best.
                        result.push_str(
                            &tags
                                .get(&substring.to_ascii_lowercase())
                                .map(|s| s.as_str())
                                .unwrap_or("???"),
                        );
                    }
                }
                c if starts == 0 => result.push(c),
                _ => (),
            },
            None => break,
        }
    }

    result
}

pub fn parse<T: AsRef<str>>(tagstring: T, tags: &Tags) -> String {
    let tagstring = tagstring.as_ref();
    let mut start = false;
    let mut iter = tagstring.chars();
    loop {
        match iter.next() {
            // makes sure there's at least one set of good braces
            Some(c) => match c {
                '\\' => drop(iter.next()),
                '<' => start = true,
                '>' if start => break parse_recurse(tagstring, tags),
                _ => (),
            },
            // else just dumb check
            None => {
                break tags
                    .get(&tagstring.to_ascii_lowercase())
                    .map(|s| s.as_str())
                    .unwrap_or("???")
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tagstring_tests {
    use super::parse;
    use super::Tags;
    fn tags() -> Tags {
        Tags::from([
            (String::from("tit1"), String::from("TheTitle")),
            (String::from("title"), String::from("TheTitle")),
            (String::from("talb"), String::from("TheAlbum")),
            (String::from("album"), String::from("TheAlbum")),
            (String::from("tcon"), String::from("TheGenre")),
            (String::from("genre"), String::from("TheGenre")),
            (String::from("goofy"), String::from("Title <GoofySpec>")),
        ])
    }

    #[test]
    fn control() {
        assert_eq!(parse("Hello, World!", &tags()), "???".to_string());
    }

    #[test]
    fn control_unequal() {
        assert_eq!(parse("Hello>, Worl<d!", &tags()), "???".to_string());
    }

    #[test]
    fn control_unequal2() {
        assert_eq!(parse(">Hello, World!<", &tags()), "???".to_string());
    }

    #[test]
    fn sub() {
        assert_eq!(parse("<title>", &tags()), "TheTitle".to_string());
    }

    #[test]
    fn sub_case() {
        assert_eq!(parse("<TiTlE>", &tags()), "TheTitle".to_string());
    }

    #[test]
    fn sub_literal() {
        assert_eq!(parse("TiT1", &tags()), "TheTitle".to_string());
    }

    #[test]
    fn sub_goofy() {
        assert_eq!(parse("<goofy>", &tags()), "Title <GoofySpec>".to_string());
    }

    #[test]
    fn sub_goofy_literal() {
        assert_eq!(parse("goofy", &tags()), "Title <GoofySpec>".to_string());
    }

    #[test]
    fn sub_before() {
        assert_eq!(
            parse("<title> is the title!", &tags()),
            "TheTitle is the title!".to_string()
        );
    }

    #[test]
    fn sub_after() {
        assert_eq!(
            parse("The title is <title>", &tags()),
            "The title is TheTitle".to_string()
        );
    }

    #[test]
    fn sub_inline() {
        assert_eq!(
            parse("This title: <title> is rad!", &tags()),
            "This title: TheTitle is rad!".to_string()
        );
    }

    #[test]
    fn sub_multi() {
        assert_eq!(
            parse("Title: <title>, Album: <album>!", &tags()),
            "Title: TheTitle, Album: TheAlbum!".to_string()
        );
    }

    #[test]
    fn escape() {
        assert_eq!(parse(r#"\<title>"#, &tags()), "???".to_string());
    }

    #[test]
    fn mixed() {
        assert_eq!(
            parse(
                r#"Title: \<title\>, Album: <<album>>>, Genre: <genre>, done!"#,
                &tags()
            ),
            r#"Title: <title>, Album: ???>, Genre: TheGenre, done!"#.to_string()
        );
    }

    #[test]
    fn condition_true() {
        assert_eq!(
            parse("Tag?<title| Title: <title>!>", &tags()),
            "Tag? Title: TheTitle!".to_string()
        );
    }

    #[test]
    fn condition_false() {
        assert_eq!(
            parse("Tag?<badtag| Badtag: <badtag>!>", &tags()),
            "Tag?".to_string()
        );
    }

    #[test]
    fn condition_invert_true() {
        assert_eq!(
            parse("Tag?<!title| Title: <title>!>", &tags()),
            "Tag?".to_string()
        );
    }

    #[test]
    fn condition_invert_false() {
        assert_eq!(
            parse("Tag?<!badtag| Badtag: <badtag>!>", &tags()),
            "Tag? Badtag: ???!".to_string()
        );
    }

    #[test]
    fn condition_mixed() {
        assert_eq!(
            parse(
                r#"<mood|This is a very <mood> song~><!mood|\<title\>: <title><TALB| is part of <TALB>>>"#,
                &tags()
            ),
            "<title>: TheTitle is part of TheAlbum".to_string()
        );
    }

    #[test]
    fn goofy_mixed() {
        assert_eq!(
            parse(
                r#"<mood|This is a very <mood> song~><!mood|\<goofy\>: <goofy><TALB| is part of <TALB>>>"#,
                &tags()
            ),
            "<goofy>: Title <GoofySpec> is part of TheAlbum".to_string()
        );
    }
}
