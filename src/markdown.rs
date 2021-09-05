use std::fmt::Write;

#[derive(Debug, Clone)]
pub enum MarkdownAst<'a> {
    Text(&'a str),
    Bold(Vec<MarkdownAst<'a>>),
    Italics(Vec<MarkdownAst<'a>>),
    BulletPoint(usize, Vec<MarkdownAst<'a>>),
    Header(usize, Vec<MarkdownAst<'a>>),
    Code(&'a str),
    Codeblock(&'a str, &'a str),
    Underline(Vec<MarkdownAst<'a>>),
    StrikeThrough(Vec<MarkdownAst<'a>>),
    Quotes(Vec<MarkdownAst<'a>>),
    Spoiler(Vec<MarkdownAst<'a>>),
    Link(Vec<MarkdownAst<'a>>, &'a str),
    Line,
}

impl<'a> MarkdownAst<'a> {
    fn get_vec(&mut self) -> &mut Vec<MarkdownAst<'a>> {
        match self {
            MarkdownAst::Text(_) => panic!(),
            MarkdownAst::Bold(v)
            | MarkdownAst::Italics(v)
            | MarkdownAst::BulletPoint(_, v)
            | MarkdownAst::Header(_, v)
            | MarkdownAst::Underline(v)
            | MarkdownAst::StrikeThrough(v)
            | MarkdownAst::Quotes(v)
            | MarkdownAst::Spoiler(v)
            | MarkdownAst::Link(v, _) => v,

            MarkdownAst::Code(_) | MarkdownAst::Codeblock(_, _) | MarkdownAst::Line => {
                panic!("unsupported")
            }
        }
    }
}

fn find<'a>(
    s: &'a str,
    finding: &str,
    not_finding: &str,
    newline_sensitive: bool,
) -> Option<&'a str> {
    let mut merged = String::with_capacity(finding.len() + not_finding.len());
    merged.push_str(finding);
    merged.push_str(not_finding);
    for i in 0..s.len() {
        if newline_sensitive && s.as_bytes()[i] == b'\n' {
            return None;
        }

        if (not_finding.is_empty()
            || !s[i..].starts_with(not_finding)
            || s[i..].starts_with(&merged))
            && s[i..].starts_with(finding)
            && (finding.len() == 1 || !s[i + 1..].starts_with(finding))
        {
            return Some(&s[..i]);
        }
    }

    None
}

fn parse_markdown_helper_helper(s: &str) -> Vec<MarkdownAst<'_>> {
    let mut vec = vec![];
    let mut i = 0;
    let mut start = 0;
    let len = s.len();
    while i < len {
        if let Some((markdown, delta)) = parse_markdown_helper(s, i) {
            if start < i {
                vec.push(MarkdownAst::Text(&s[start..i]));
            }

            vec.push(markdown);
            i += delta;
            start = i;
        } else {
            i += 1;
        }
    }

    if start < len {
        vec.push(MarkdownAst::Text(&s[start..len]));
    }

    vec
}

fn parse_markdown_helper(s: &str, i: usize) -> Option<(MarkdownAst<'_>, usize)> {
    let bytes = s.as_bytes();
    let c = bytes[i] as char;
    let len = s.len();
    match c {
        '*' if len > 2 && i < len - 2 && bytes[i + 1] == b'*' => {
            if let Some(sub) = find(&s[i + 2..], "**", "", true) {
                let bold = MarkdownAst::Bold(parse_markdown_helper_helper(sub));
                Some((bold, sub.len() + 4))
            } else {
                None
            }
        }

        '*' => {
            if let Some(sub) = find(&s[i + 1..], "*", "**", true) {
                let italics = MarkdownAst::Italics(parse_markdown_helper_helper(sub));
                Some((italics, sub.len() + 2))
            } else {
                None
            }
        }

        '_' if len > 2 && i < len - 2 && bytes[i + 1] == b'_' => {
            if let Some(sub) = find(&s[i + 2..], "__", "", true) {
                let underline = MarkdownAst::Underline(parse_markdown_helper_helper(sub));
                Some((underline, sub.len() + 4))
            } else {
                None
            }
        }

        '_' => {
            if let Some(sub) = find(&s[i + 1..], "_", "__", true) {
                let italics = MarkdownAst::Italics(parse_markdown_helper_helper(sub));
                Some((italics, sub.len() + 2))
            } else {
                None
            }
        }

        '~' if len > 2 && i < len - 2 && bytes[i + 1] == b'~' => {
            if let Some(sub) = find(&s[i + 2..], "~~", "", true) {
                let strike = MarkdownAst::StrikeThrough(parse_markdown_helper_helper(sub));
                Some((strike, sub.len() + 4))
            } else {
                None
            }
        }

        '`' if len > 3 && i < len - 3 && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' => {
            if let Some(sub) = find(&s[i + 3..], "```", "", false) {
                let (type_, block) = match sub.find('\n') {
                    Some(i) => (&sub[..i], &sub[i + 1..]),
                    None => ("", sub),
                };
                let block = MarkdownAst::Codeblock(type_, block);
                Some((block, sub.len() + 6))
            } else {
                None
            }
        }

        '`' => {
            if let Some(sub) = find(&s[i + 1..], "`", "", true) {
                let code = MarkdownAst::Code(sub);
                Some((code, sub.len() + 2))
            } else {
                None
            }
        }

        '|' if len > 2 && i < len - 2 && bytes[i + 1] == b'|' => {
            if let Some(sub) = find(&s[i + 2..], "||", "", true) {
                let spoiler = MarkdownAst::Spoiler(parse_markdown_helper_helper(sub));
                Some((spoiler, sub.len() + 4))
            } else {
                None
            }
        }

        '[' => {
            if let Some(sub) = find(&s[i + 1..], "]", "", false) {
                let v = parse_markdown_helper_helper(sub);
                if i + sub.len() < len - 3 && s.as_bytes()[i + sub.len() + 2] == b'(' {
                    if let Some(link) = find(&s[i + sub.len() + 3..], ")", "", false) {
                        let len = link.len();
                        let link = MarkdownAst::Link(v, link);
                        Some((link, sub.len() + 4 + len))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }

        _ => None,
    }
}

pub fn parse_markdown(s: &str) -> Vec<MarkdownAst<'_>> {
    let mut vec = vec![];
    let mut newline = true;
    let mut i = 0;
    let mut start = 0;
    let len = s.len();
    let bytes = s.as_bytes();
    let mut wrapper = None;
    while i < len {
        if bytes[i] > b'\x7f' {
            i += 1;
            continue;
        }

        let c = bytes[i] as char;
        if newline {
            match c {
                '#' => {
                    start = i;
                    while bytes[i + 1] == b'#' {
                        i += 1;
                    }
                    wrapper = Some(MarkdownAst::Header(i - start + 1, vec![]));
                    i += 1;
                    start = i;
                    continue;
                }

                '>' if len > 2 && i < len - 2 && matches!(bytes[i + 1], b' ' | b'\t') => {
                    wrapper = Some(MarkdownAst::Quotes(vec![]));
                    i += 1;
                    start = i;
                    continue;
                }

                '-' if len > 2 && i < len - 2 && bytes[i + 1] == b'-' && bytes[i + 2] == b'-' => {
                    match (
                        s[i + 3..].find('\n'),
                        s[i + 3..].find(|v: char| !v.is_whitespace()),
                    ) {
                        (Some(jj), Some(kk)) if jj < kk => {
                            i += jj + 4;
                            start = i;
                            vec.push(MarkdownAst::Line);
                            continue;
                        }

                        (Some(j), None) => {
                            i += j + 4;
                            start = i;
                            vec.push(MarkdownAst::Line);
                            continue;
                        }

                        _ => (),
                    }
                }

                '-' => {
                    wrapper = Some(MarkdownAst::BulletPoint(i - start, vec![]));
                    i += 1;
                    start = i;
                    continue;
                }

                _ => (),
            }

            newline = c == ' ' || c == '\n' || c == '\t';
        } else if c == '\n' {
            newline = true;
            if let Some(mut wrapper) = wrapper {
                if start < i {
                    wrapper.get_vec().push(MarkdownAst::Text(&s[start..i]));
                    start = i;
                }
                vec.push(wrapper);
            }
            wrapper = None;
        }

        if let Some((markdown, delta)) = parse_markdown_helper(s, i) {
            match &mut wrapper {
                Some(v) => {
                    let vec = v.get_vec();
                    vec.push(MarkdownAst::Text(&s[start..i]));
                    vec.push(markdown);
                }

                None => {
                    vec.push(MarkdownAst::Text(&s[start..i]));
                    vec.push(markdown);
                }
            }

            i += delta;
            start = i;
        } else {
            i += 1;
        }
    }

    if start < len {
        match wrapper {
            Some(mut v) => {
                {
                    let vec = v.get_vec();
                    vec.push(MarkdownAst::Text(&s[start..len]));
                }
                vec.push(v);
            }

            None => {
                vec.push(MarkdownAst::Text(&s[start..len]));
            }
        }
    }

    vec
}

fn markdown_to_html_helper(s: &mut String, ast: MarkdownAst) -> Result<(), std::fmt::Error> {
    match ast {
        MarkdownAst::Text(t) => write!(s, "{}", t),

        MarkdownAst::Bold(v) => {
            write!(s, "<strong>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</strong>")
        }

        MarkdownAst::Italics(v) => {
            write!(s, "<em>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</em>")
        }

        MarkdownAst::BulletPoint(_, v) => {
            write!(s, "<li>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</li>")
        }

        MarkdownAst::Header(i, v) => {
            write!(s, "<h{}>", i)?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</h{}>", i)
        }

        MarkdownAst::Code(v) => write!(s, "<code>{}</code>", v),

        // TODO: use the language field
        MarkdownAst::Codeblock(_, v) => write!(s, "<pre><code>{}</code></pre>", v),

        MarkdownAst::Underline(v) => {
            write!(s, "<u>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</u>")
        }

        MarkdownAst::StrikeThrough(v) => {
            write!(s, "<del>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</del>")
        }

        MarkdownAst::Quotes(v) => {
            write!(s, "<blockquote>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</blockquote>")
        }

        MarkdownAst::Spoiler(v) => {
            write!(s, "<span data-mx-spoiler>")?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</span>")
        }

        MarkdownAst::Link(v, href) => {
            write!(s, "<a href={:?}>", href)?;
            for v in v {
                markdown_to_html_helper(s, v)?;
            }
            write!(s, "</a>")
        }

        MarkdownAst::Line => write!(s, "<hr>"),
    }
}

pub fn markdown_to_html(asts: Vec<MarkdownAst>) -> String {
    let mut s = String::new();
    let mut depths = vec![];
    for a in asts {
        if let MarkdownAst::BulletPoint(i, _) = a {
            match depths.last() {
                Some(j) if i < *j => {
                    let _ = write!(s, "</ul>");
                    while matches!(depths.last(), Some(j) if i < *j) {
                        depths.pop();
                    }
                }

                Some(j) if i > *j => {
                    let _ = write!(s, "<ul>");
                    depths.push(i);
                }

                Some(_) => (),

                None => {
                    let _ = write!(s, "<ul>");
                    depths.push(i);
                }
            }
        } else if !depths.is_empty() {
            while depths.pop().is_some() {
                let _ = write!(s, "</ul>");
            }
        }

        let _ = markdown_to_html_helper(&mut s, a);
    }
    while depths.pop().is_some() {
        let _ = write!(s, "</ul>");
    }
    s
}
