use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateScope {
    Global,
    Connection(String),
}

impl TemplateScope {
    pub fn matches(&self, connection_name: &str) -> bool {
        match self {
            TemplateScope::Global => true,
            TemplateScope::Connection(name) => name == connection_name,
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
        dirs::config_dir().map(|d| d.join("sqli").join("templates.sql"))
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
            if line.starts_with("--- ") {
                // Save previous template if exists
                if let (Some(name), Some(scope)) = (current_name.take(), current_scope.take()) {
                    let query = current_query.trim().to_string();
                    if !query.is_empty() {
                        templates.push(Template { name, query, scope });
                    }
                    current_query.clear();
                }

                // Parse new template header: "--- Name [scope]"
                let header = &line[4..]; // Skip "--- "
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
    fn parse_header(header: &str) -> Option<(String, TemplateScope)> {
        let header = header.trim();
        if let Some(bracket_start) = header.rfind('[') {
            if let Some(bracket_end) = header.rfind(']') {
                if bracket_end > bracket_start {
                    let name = header[..bracket_start].trim().to_string();
                    let scope_str = &header[bracket_start + 1..bracket_end];
                    let scope = if scope_str.eq_ignore_ascii_case("global") {
                        TemplateScope::Global
                    } else {
                        TemplateScope::Connection(scope_str.to_string())
                    };
                    return Some((name, scope));
                }
            }
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
            TemplateScope::Connection(name) => name.clone(),
        };
        format!("--- {} [{}]\n{}\n", template.name, scope_str, template.query)
    }

    /// Parse a single template from content (used for editing)
    pub fn parse_one(content: &str) -> Option<Template> {
        Self::parse(content).into_iter().next()
    }
}

/// Find the first <placeholder> in a query and return (line, col, length)
pub fn find_placeholder(query: &str) -> Option<(usize, usize, usize)> {
    for (line_idx, line) in query.lines().enumerate() {
        if let Some(start) = line.find('<') {
            if let Some(end_offset) = line[start..].find('>') {
                // Length includes < and >
                return Some((line_idx, start, end_offset + 1));
            }
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
SELECT COUNT(*) FROM <table>

--- Active Users [my-db]
SELECT * FROM users
WHERE active = true
LIMIT <limit>
"#;

        let templates = TemplateStore::parse(content);
        assert_eq!(templates.len(), 2);

        assert_eq!(templates[0].name, "Count Rows");
        assert_eq!(templates[0].scope, TemplateScope::Global);
        assert_eq!(templates[0].query, "SELECT COUNT(*) FROM <table>");

        assert_eq!(templates[1].name, "Active Users");
        assert_eq!(
            templates[1].scope,
            TemplateScope::Connection("my-db".to_string())
        );
        assert!(templates[1].query.contains("WHERE active = true"));
    }

    #[test]
    fn test_serialize_templates() {
        let templates = vec![
            Template {
                name: "Test".to_string(),
                query: "SELECT 1".to_string(),
                scope: TemplateScope::Global,
            },
            Template {
                name: "Local".to_string(),
                query: "SELECT 2".to_string(),
                scope: TemplateScope::Connection("db".to_string()),
            },
        ];

        let output = TemplateStore::serialize(&templates);
        assert!(output.contains("--- Test [global]"));
        assert!(output.contains("--- Local [db]"));
    }

    #[test]
    fn test_find_placeholder() {
        let query = "SELECT * FROM <table> WHERE id = <id>";
        let result = find_placeholder(query);
        assert_eq!(result, Some((0, 14, 7))); // <table> starts at col 14, length 7

        let multiline = "SELECT *\nFROM <table>";
        let result = find_placeholder(multiline);
        assert_eq!(result, Some((1, 5, 7))); // <table> on line 1

        let no_placeholder = "SELECT * FROM users";
        assert_eq!(find_placeholder(no_placeholder), None);
    }

    #[test]
    fn test_scope_matches() {
        assert!(TemplateScope::Global.matches("any-db"));
        assert!(TemplateScope::Connection("my-db".to_string()).matches("my-db"));
        assert!(!TemplateScope::Connection("my-db".to_string()).matches("other-db"));
    }
}
