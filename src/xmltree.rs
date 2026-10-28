//! Small DOM for viewing arbitrary XML files (system info, registry exports...).

use quick_xml::events::{BytesStart, Event as XmlEvent};
use quick_xml::Reader;
use std::io::BufRead;

#[derive(Clone, Debug, Default)]
pub struct XmlNode {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<XmlNode>,
}

impl XmlNode {
    pub fn from_start(e: &BytesStart) -> Self {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let attrs = e
            .attributes()
            .flatten()
            .map(|a| {
                (
                    String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                    a.unescape_value().map(|v| v.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned()),
                )
            })
            .collect();
        XmlNode { name, attrs, text: String::new(), children: vec![] }
    }

    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    #[allow(dead_code)]
    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.name == name)
    }

    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(|c| c.count()).sum::<usize>()
    }
}

/// Parses a whole XML document into a tree (root = synthetic "#document").
#[allow(dead_code)]
pub fn parse<R: BufRead>(r: R) -> Result<XmlNode, String> {
    let mut reader = Reader::from_reader(r);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<XmlNode> = vec![XmlNode { name: "#document".into(), ..Default::default() }];
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) => stack.push(XmlNode::from_start(&e)),
            Ok(XmlEvent::Empty(e)) => {
                let n = XmlNode::from_start(&e);
                stack.last_mut().unwrap().children.push(n);
            }
            Ok(XmlEvent::End(_)) => {
                if stack.len() > 1 {
                    let n = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(n);
                }
            }
            Ok(XmlEvent::Text(t)) => {
                let s = t.unescape().map(|s| s.into_owned()).unwrap_or_default();
                let top = stack.last_mut().unwrap();
                if !top.text.is_empty() {
                    top.text.push(' ');
                }
                top.text.push_str(s.trim());
            }
            Ok(XmlEvent::CData(t)) => {
                let top = stack.last_mut().unwrap();
                top.text.push_str(&String::from_utf8_lossy(&t));
            }
            Ok(XmlEvent::Eof) => break,
            Err(e) => {
                if stack.len() == 1 && stack[0].children.is_empty() {
                    return Err(format!("XML error at position {}: {}", reader.buffer_position(), e));
                }
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(n);
    }
    Ok(stack.pop().unwrap())
}
