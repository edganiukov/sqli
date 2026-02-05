use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateScope {
    Global,
    Connections(Vec<String>),
}

impl TemplateScope {
    pub fn matches(&self, connection_name: &str) -> bool {
        match self {
            TemplateScope::Global => true,
            TemplateScope::Connections(names) => names.iter().any(|n| n == connection_name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub query: String,
    pub scope: TemplateScope,
}

#[derive(Debug, Default)]
pub struct TemplateStore {
    pub templates: Vec<Template>,
}

impl TemplateStore {
    pub fn load() -> Self {
        let path = Self::get_templates_path();
        match path {
            Some(p) if p.exists() => match fs::read_to_string(&p) {
                Ok(content) => Self {
                    templates: Self::parse(&content),
                },
                Err(_) => Self::default(),
            },
            _ => Self::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::get_templates_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine config directory",
            )
        })?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = Self::serialize(&self.templates);
        fs::write(path, content)
    }

    fn get_templates_path() -> Option<PathBuf> {
        // Use ~/.config on all platforms for consistency
        dirs::home_dir().map(|h| h.join(".config").join("sqli").join("templates.sql"))
    }

    pub fn get_templates_for_connection(&self, connection_name: &str) -> Vec<&Template> {
        self.templates
            .iter()
            .filter(|t| t.scope.matches(connection_name))
            .collect()
    }

    pub fn add_template(&mut self, template: Template) {
        self.templates.push(template);
    }

    pub fn delete_template(&mut self, index: usize) -> bool {
        if index < self.templates.len() {
            self.templates.remove(index);
            true
        } else {
            false
        }
    }

    /// Parse templates from the simple delimiter format:
    /// --- Template Name [scope]
    /// query content here
    /// can be multiline
    ///
    /// --- Another Template [global]
    /// SELECT * FROM table
    fn parse(content: &str) -> Vec<Template> {
        let mut templates = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_scope: Option<TemplateScope> = None;
        let mut current_query = String::new();

        for line in content.lines() {
            if let Some(header) = line.strip_prefix("--- ") {
                // Save previous template if exists
                if let (Some(name), Some(scope)) = (current_name.take(), current_scope.take()) {
                    let query = current_query.trim().to_string();
                    if !query.is_empty() {
                        templates.push(Template { name, query, scope });
                    }
                    current_query.clear();
                }

                // Parse new template header: "--- Name [scope]"
                if let Some((name, scope)) = Self::parse_header(header) {
                    current_name = Some(name);
                    current_scope = Some(scope);
                }
            } else if current_name.is_some() {
                // Accumulate query lines
                if !current_query.is_empty() {
                    current_query.push('\n');
                }
                current_query.push_str(line);
            }
        }

        // Don't forget the last template
        if let (Some(name), Some(scope)) = (current_name, current_scope) {
            let query = current_query.trim().to_string();
            if !query.is_empty() {
                templates.push(Template { name, query, scope });
            }
        }

        templates
    }

    /// Parse header line: "Template Name [scope]" -> (name, scope)
    /// Supports: [global], [conn], [conn1,conn2,conn3]
    fn parse_header(header: &str) -> Option<(String, TemplateScope)> {
        let header = header.trim();
        let bracket_start = header.rfind('[')?;
        let bracket_end = header.rfind(']')?;
        if bracket_end > bracket_start {
            let name = header[..bracket_start].trim().to_string();
            let scope_str = &header[bracket_start + 1..bracket_end];
            let scope = if scope_str.eq_ignore_ascii_case("global") {
                TemplateScope::Global
            } else {
                // Parse comma-separated connection names
                let connections: Vec<String> = scope_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                TemplateScope::Connections(connections)
            };
            return Some((name, scope));
        }
        None
    }

    /// Serialize templates to the simple delimiter format
    fn serialize(templates: &[Template]) -> String {
        templates.iter().map(Self::serialize_one).collect()
    }

    /// Serialize a single template to the delimiter format
    pub fn serialize_one(template: &Template) -> String {
        let scope_str = match &template.scope {
            TemplateScope::Global => "global".to_string(),
            TemplateScope::Connections(names) => names.join(","),
        };
        format!(
            "--- {} [{}]\n{}\n",
            template.name, scope_str, template.query
        )
    }

    /// Parse a single template from content (used for editing)
    pub fn parse_one(content: &str) -> Option<Template> {
        Self::parse(content).into_iter().next()
    }
}

/// Find the first <placeholder> in a query and return (line, col, length)
pub fn find_placeholder(query: &str) -> Option<(usize, usize, usize)> {
    for (line_idx, line) in query.lines().enumerate() {
        if let Some(start) = line.find('<')
            && let Some(end_offset) = line[start..].find('>')
        {
            // Length includes < and >
            return Some((line_idx, start, end_offset + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_templates() {
        let content = r#"--- Count Rows [global]
select count(*) from <table>

--- Active Users [my-db]
select * from users
where active = true
limit <limit>
"#;

        let templates = TemplateStore::parse(content);
        assert_eq!(templates.len(), 2);

        assert_eq!(templates[0].name, "Count Rows");
        assert_eq!(templates[0].scope, TemplateScope::Global);
        assert_eq!(templates[0].query, "select count(*) from <table>");

        assert_eq!(templates[1].name, "Active Users");
        assert_eq!(
            templates[1].scope,
            TemplateScope::Connections(vec!["my-db".into()])
        );
        assert!(templates[1].query.contains("where active = true"));
    }

    #[test]
    fn test_parse_multi_connection() {
        let content = "--- Shared Query [db1,db2,db3]\nselect 1\n";
        let templates = TemplateStore::parse(content);
        assert_eq!(templates.len(), 1);
        assert_eq!(
            templates[0].scope,
            TemplateScope::Connections(vec!["db1".into(), "db2".into(), "db3".into()])
        );
        // Should match any of the listed connections
        assert!(templates[0].scope.matches("db1"));
        assert!(templates[0].scope.matches("db2"));
        assert!(templates[0].scope.matches("db3"));
        assert!(!templates[0].scope.matches("db4"));
    }

    #[test]
    fn test_serialize_templates() {
        let templates = vec![
            Template {
                name: "Test".to_string(),
                query: "select 1".to_string(),
                scope: TemplateScope::Global,
            },
            Template {
                name: "Local".to_string(),
                query: "select 2".to_string(),
                scope: TemplateScope::Connections(vec!["db".into()]),
            },
            Template {
                name: "Multi".to_string(),
                query: "select 3".to_string(),
                scope: TemplateScope::Connections(vec!["a".into(), "b".into()]),
            },
        ];

        let output = TemplateStore::serialize(&templates);
        assert!(output.contains("--- Test [global]"));
        assert!(output.contains("--- Local [db]"));
        assert!(output.contains("--- Multi [a,b]"));
    }

    #[test]
    fn test_find_placeholder() {
        let query = "select * from <table> where id = <id>";
        let result = find_placeholder(query);
        assert_eq!(result, Some((0, 14, 7))); // <table> starts at col 14, length 7

        let multiline = "select *\nfrom <table>";
        let result = find_placeholder(multiline);
        assert_eq!(result, Some((1, 5, 7))); // <table> on line 1

        let no_placeholder = "select * from users";
        assert_eq!(find_placeholder(no_placeholder), None);
    }

    #[test]
    fn test_scope_matches() {
        assert!(TemplateScope::Global.matches("any-db"));
        assert!(TemplateScope::Connections(vec!["my-db".into()]).matches("my-db"));
        assert!(!TemplateScope::Connections(vec!["my-db".into()]).matches("other-db"));
        // Multi-connection scope
        assert!(TemplateScope::Connections(vec!["a".into(), "b".into()]).matches("a"));
        assert!(TemplateScope::Connections(vec!["a".into(), "b".into()]).matches("b"));
        assert!(!TemplateScope::Connections(vec!["a".into(), "b".into()]).matches("c"));
    }
}
