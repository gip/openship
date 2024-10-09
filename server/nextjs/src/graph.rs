use crate::hash::{AbsHash, ImplHash};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{HashMap, HashSet};

// A minimal and naive file-based dependency graph implementation

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct Object(pub String);
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct Scope(pub String);
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct Version(pub String);
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct Extension(pub String);
#[derive(PartialEq, Eq, Clone, Hash, Debug, Serialize, Deserialize)]
pub struct Mangled(pub String);

#[derive(PartialEq, Clone, Serialize, Deserialize)]
pub struct Node {
    pub o: Object,
    pub s: Scope,
    pub e: Option<Extension>,
    pub a: AbsHash,
    pub i: Option<ImplHash>,
    pub d: HashSet<Mangled>,
    pub v: Option<Version>,
}

pub struct Graph {
    existing: HashMap<Mangled, Node>,
    new: HashMap<Mangled, Node>,
}

impl Serialize for Graph {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Graph", 1)?;
        let all_nodes: Vec<_> = self.existing.iter().chain(self.new.iter()).collect();
        state.serialize_field("nodes", &all_nodes)?;
        state.end()
    }
}

impl Graph {
    #[allow(dead_code)]
    pub fn new() -> Graph {
        let existing = HashMap::new();
        let new = HashMap::new();
        Graph { existing, new }
    }
    pub fn read_graph<'a>(it: impl Iterator<Item = &'a str>) -> serde_json::Result<Graph> {
        let mut existing = HashMap::new();
        for line in it {
            let node: Node = serde_json::from_str(&line)?;
            let key = Self::mangle(&node.s, &node.o, &node.e);
            existing.insert(key, node);
        }
        let new = HashMap::new();
        Ok(Graph { existing, new })
    }
    pub fn write_graph(&self) -> Vec<String> {
        self.new
            .iter()
            .map(|(_, n)| serde_json::to_string(n).unwrap())
            .collect() // How can to_json fail?
    }
    pub fn get(&mut self, k: &Mangled) -> Option<&Node> {
        match self.new.get(k) {
            Some(v) => Some(v),
            None => self.existing.get(k),
        }
    }
    pub fn insert(&mut self, v: Node) -> bool {
        let k = Self::mangle(&v.s, &v.o, &v.e);
        match self.get(&k) {
            Some(v0) => {
                if v == *v0 {
                    return false;
                }
            }
            None => (),
        };
        self.new.insert(k, v);
        true
    }
    pub fn find_with_dep<'a>(&'a self, dep: Mangled) -> Vec<&'a Node> {
        let mut vec: Vec<&'a Node> = vec![];
        for (_, n) in &self.new {
            if n.d.get(&dep).is_some() {
                vec.push(n);
            }
        }
        for (_, n) in &self.existing {
            if n.d.get(&dep).is_some() {
                vec.push(n);
            }
        }
        vec
    }
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.existing.len() + self.new.len()
    }
    pub fn mangle(scope: &Scope, object: &Object, extension: &Option<Extension>) -> Mangled {
        let Scope(scope) = scope;
        let Object(object) = object;
        let mut ext = "";
        if scope == "dep" {
            // Pass
        } else {
            ext = match extension {
                Some(Extension(e)) if e == "js" || e == "jsx" || e == "ts" || e == "tsx" => "::js",
                Some(Extension(e)) if e == "css" => "::css",
                _ => "".into(),
            };
        }
        Mangled(format!("{}::{}{}", scope, object, ext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]

    fn test_read_graph() {
        let json_data = r#"{ "v": null, "o": "o1", "e": null, "s": "s1", "a": "123", "d": [], "l": [] }
        { "v": null, "o": "o2", "e": "ts", "s": "s2", "a": "456", "d": [], "l": ["AppDir"] }
        { "v": null, "o": "o2", "e": "tsx", "s": "s2", "a": "456", "d": [], "l": ["AppDir"] }"#;
        let r = Graph::read_graph(json_data.lines());
        assert!(r.is_ok());
        assert_eq!(r.unwrap().len(), 2);
    }
}
