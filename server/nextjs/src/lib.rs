use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::{fs, io};
use swc_common::{input::StringInput, sync::Lrc, FileName, SourceMap};
use swc_core::ecma::codegen::{text_writer::JsWriter, Emitter};
use swc_core::ecma::{
    ast::Program,
    ast::{Module, ModuleDecl, ModuleItem},
};
use swc_core::plugin::{
    metadata::TransformPluginMetadataContextKind, plugin_transform,
    proxies::TransformPluginProgramMetadata,
};
use swc_ecma_parser::{error::Error as SwcError, lexer::Lexer, EsSyntax, Parser, Syntax};

mod graph;
use graph::{Extension, Graph, Mangled, Node, Object, Scope, Version};
mod hash;
use hash::{depencency_hash, program_hash, program_impl_hash, AbsHash, ImplHash};
mod path;
use path::format_dependency;

fn load_package(path: &str) -> Result<(String, String), io::Error> {
    let full_path = format!("/cwd/{}/package.json", path);
    let json_content = fs::read_to_string(&full_path)?;
    let json_data: Value = serde_json::from_str(&json_content)?;
    if let Some(version) = json_data["version"].as_str() {
        if let Some(name) = json_data["name"].as_str() {
            return Ok((name.to_string(), version.to_string()));
        }
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "Version not found"))
}

fn program_to_string(program: &Program) -> String {
    let mut buf = vec![];
    let cm: Lrc<SourceMap> = Default::default();
    let writer = JsWriter::new(cm.clone(), "\n", &mut buf, None);
    let mut emitter = Emitter {
        cfg: swc_core::ecma::codegen::Config::default(),
        cm: cm,
        comments: None,
        wr: writer,
    };

    emitter.emit_program(&program).unwrap();
    String::from_utf8(buf).unwrap()
}

fn parse_module(code: &str) -> Result<Module, SwcError> {
    let cm: Lrc<SourceMap> = Default::default();
    let source_file = cm.new_source_file(FileName::Custom("input.js".into()).into(), code.into());
    let input = StringInput::from(&*source_file);
    let lexer = Lexer::new(
        Syntax::Es(EsSyntax {
            jsx: true, // Enable JSX if needed
            ..Default::default()
        }),
        Default::default(),
        input,
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let module = parser.parse_module()?;
    Ok(module)
}

fn ensure_dir_exists() -> std::io::Result<()> {
    let path = "/cwd/.next/openship";
    if !fs::metadata(path).is_ok() {
        let _ = fs::create_dir(path);
    };
    Ok(())
}

fn open_file_with_retry(path: &Path, append: bool) -> io::Result<fs::File> {
    let mut retries = 0;
    let max_retries = 8;
    let retry_delay = Duration::from_millis(100);
    loop {
        let r = if append {
            fs::OpenOptions::new().append(true).create(true).open(path)
        } else {
            fs::OpenOptions::new().write(true).create(true).open(path)
        };
        match r {
            Ok(file) => return Ok(file),
            Err(_) if retries < max_retries => {
                retries += 1;
                thread::sleep(retry_delay);
            }
            Err(e) => return Err(e),
        }
    }
}

fn extract_import(program: &Program) -> Vec<String> {
    let body = match program {
        Program::Module(m) => &m.body,
        Program::Script(_) => return vec![],
    };
    body.into_iter()
        .filter_map(|stat| match stat {
            ModuleItem::ModuleDecl(decl) => match decl {
                ModuleDecl::Import(import_decl) => Some(import_decl.src.value.to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn lines_from_file(path: &Path) -> io::Result<Vec<String>> {
    let file_result = fs::OpenOptions::new().read(true).open(path);
    let mut file = match file_result {
        Err(_) => return Ok(vec![]), // Check file not found
        Ok(file) => file,
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let lines = contents.lines().map(|l| l.to_string()).collect();
    Ok(lines)
}

fn lines_to_file(path: &Path, append: bool, lines: Vec<String>) -> io::Result<Vec<String>> {
    let mut file = open_file_with_retry(path, append)?;
    let contents = lines
        .iter()
        .map(|l| l.to_string() + "\n")
        .collect::<String>();
    if append {
        file.seek(SeekFrom::End(0))?;
    }
    file.write_all(contents.as_bytes())?;
    Ok(lines)
}

fn handle_node(graph: &mut Graph, mut node: Node) {
    // Downstream first
    let deps = &node.d;
    let mut deps_map = HashMap::<Mangled, ImplHash>::new();
    let mut can_oshi = true;
    for dep in deps {
        match graph.get(&dep) {
            Some(n) => match &n.i {
                Some(oshi) => {
                    deps_map.insert(dep.clone(), oshi.clone());
                }
                None => {
                    can_oshi = false;
                    println!("DEP {:?} no oshi", dep);
                }
            },
            None => {
                can_oshi = false;
                println!("DEQ {:?} not found", dep);
            }
        };
    }
    println!("HAN {:?} {}", node.o, can_oshi);
    if can_oshi {
        let impl_hash = program_impl_hash(&node.a, deps_map);
        node.i = Some(impl_hash);
    };
    let mangled = Graph::mangle(&node.s, &node.o, &node.e);
    let inserted = graph.insert(node);
    // Propagate to upstream nodes
    if inserted {
        let nodes: Vec<Node> = graph
            .find_with_dep(mangled)
            .iter()
            .map(|&node| node.clone())
            .collect();
        for n in nodes {
            handle_node(graph, n.clone());
        }
    }
}

fn process(
    program: Program,
    metadata: TransformPluginProgramMetadata,
) -> Result<Program, io::Error> {
    let cwd = metadata
        .get_context(&TransformPluginMetadataContextKind::Cwd)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cwd can't be retrieved"))?;
    let _env = metadata.get_context(&TransformPluginMetadataContextKind::Env);
    match ensure_dir_exists() {
        Err(err) => return Err(err),
        Ok(_) => (),
    };
    let graph_path = Path::new("/cwd/.next/openship/graph");
    let graph_lines = lines_from_file(graph_path)?;
    let mut graph = Graph::read_graph(graph_lines.iter().map(|l| l.as_str()))?;

    let file_name = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "filepath can't be retrieved"))?;
    let full_path = Path::new(&file_name);
    let path = full_path
        .strip_prefix(cwd)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "wrong prefix"))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    if let Some(_extension) = path.extension() {
        if path.to_string_lossy() == "app/.openship/route.ts" {
            let (name, version) = load_package("")?;
            let code = include_str!("./route.ts");
            let code = code.replace("{application}", &name);
            let code = code.replace("{version}", &version);
            let module = parse_module(&code)
                .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "couldn't parse"))?;
            return Ok(Program::Module(module));
        };
        if path.starts_with("node_modules/") {
            if let Some(c) = path.components().nth(1) {
                if let Some(name) = c.as_os_str().to_str() {
                    let name = format!("node_modules/{}", name);
                    let (name, version) = load_package(&name)?;
                    let extension = None;
                    let object = Object(name.clone());
                    let scope = Scope("dep".into());
                    let (abs_hash, impl_hash) = depencency_hash(&name, &version);
                    let version = Some(Version(version));
                    let node = Node {
                        o: object,
                        e: extension,
                        s: scope,
                        a: abs_hash,
                        i: Some(impl_hash),
                        d: HashSet::new(),
                        v: version,
                    };
                    graph.insert(node);
                }
            }
        } else {
            let path_no_extension = PathBuf::from(path).with_extension("");
            let object = Object(path_no_extension.display().to_string());
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| Extension(s.to_string()));
            let scope = Scope("app".to_string());
            let abstract_hash = program_hash(&program);
            let imports = extract_import(&program);
            let version = None;
            let d = imports
                .iter()
                .map(|i| {
                    let (scope, object, extension) = format_dependency(&dir, &i);
                    Graph::mangle(&scope, &object, &extension)
                })
                .collect();
            let node = Node {
                o: object,
                e: extension,
                s: scope,
                a: abstract_hash.clone(),
                i: None,
                d,
                v: version,
            };
            handle_node(&mut graph, node);
            let program_string = program_to_string(&program);
            let AbsHash(hash_string) = abstract_hash;
            let path = format!("/cwd/.next/openship/{}", hash_string);
            let path = Path::new(&path);
            let _ = lines_to_file(&path, false, vec![program_string]);
        };
    };
    match lines_to_file(graph_path, true, graph.write_graph()) {
        Ok(_) => (),
        Err(err) => println!("XEE {}", err),
    }
    Ok(program)
}

#[plugin_transform]
pub fn process_transform(program: Program, metadata: TransformPluginProgramMetadata) -> Program {
    match process(program, metadata) {
        Ok(p) => p,
        Err(err) => {
            println!("OpenShip SWC plugin error");
            println!("{}", err.to_string());
            panic!("panic: {}", err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node(name: &str, deps: Vec<Mangled>) -> Node {
        Node {
            o: Object(name.to_string()),
            e: Some(Extension("js".to_string())),
            s: Scope("".to_string()),
            a: AbsHash(format!("hash_{}", name)),
            i: None,
            d: deps.into_iter().collect(),
            v: None,
        }
    }

    #[test]
    fn test_handle_node_no_deps() {
        let mut graph = Graph::new();
        let node = create_test_node("A", vec![]);

        handle_node(&mut graph, node.clone());

        let handled = graph
            .get(&Graph::mangle(&node.s, &node.o, &node.e))
            .unwrap();
        assert!(handled.i.is_some());
    }

    // #[test]
    // fn test_handle_node_with_missing_dep() {
    //     let mut graph = Graph::new();
    //     let dep_mangled = Graph::mangle(&Scope("".to_string()), &Object("B".to_string()));
    //     let node = create_test_node("A", vec![dep_mangled]);

    //     handle_node(&mut graph, node.clone());

    //     let handled = graph.get(&Graph::mangle(&node.s, &node.o, &node.e)).unwrap();
    //     assert!(handled.i.is_none());
    // }

    #[test]
    fn test_handle_node_with_existing_dep() {
        let mut graph = Graph::new();
        let dep_node = create_test_node("B", vec![]);
        graph.insert(dep_node.clone());
        handle_node(&mut graph, dep_node.clone()); // Ensure B has an impl hash

        let dep_mangled = Graph::mangle(&dep_node.s, &dep_node.o, &dep_node.e);
        let node = create_test_node("A", vec![dep_mangled]);

        handle_node(&mut graph, node.clone());

        let handled = graph
            .get(&Graph::mangle(&node.s, &node.o, &node.e))
            .unwrap();
        assert!(handled.i.is_some());
    }

    #[test]
    fn test_handle_node_update_propagation() {
        let mut graph = Graph::new();

        // Create and handle B first
        let node_b = create_test_node("B", vec![]);
        handle_node(&mut graph, node_b.clone());

        // Create A depending on B
        let dep_mangled = Graph::mangle(&node_b.s, &node_b.o, &node_b.e);
        let node_a = create_test_node("A", vec![dep_mangled]);
        graph.insert(node_a.clone());

        // Update B and handle it
        let mut updated_b = node_b.clone();
        updated_b.a = AbsHash("new_hash_B".to_string());
        handle_node(&mut graph, updated_b);

        // Check if A's impl hash has changed
        let updated_a = graph
            .get(&Graph::mangle(&node_a.s, &node_a.o, &node_a.e))
            .unwrap();
        assert!(updated_a.i.is_some());
        assert_ne!(updated_a.i, node_a.i);
    }
}
