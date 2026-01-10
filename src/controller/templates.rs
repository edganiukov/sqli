use super::{Controller, PopupState};
use crate::templates::{Template, TemplateScope, TemplateStore};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::CursorMove;

impl Controller {
    pub(super) fn open_template_popup(&mut self) {
        let conn_name = self.current_tab().connected_db.as_deref().unwrap_or("");

        self.template_list_cache = self
            .template_store
            .get_templates_for_connection(conn_name)
            .into_iter()
            .cloned()
            .collect();

        if self.template_list_cache.is_empty() {
            self.current_tab_mut().status_message =
                Some("No templates saved. Use Ctrl+S to save a template.".to_string());
            return;
        }

        self.popup_state = PopupState::TemplateList { selected: 0 };
    }

    pub(super) fn open_save_template_popup(&mut self) {
        let query: String = self.query_textarea.lines().join("\n");
        if query.trim().is_empty() {
            self.current_tab_mut().status_message =
                Some("Cannot save empty query as template".to_string());
            return;
        }

        // Default scope based on current connection
        let scope = match &self.current_tab().connected_db {
            Some(conn) => TemplateScope::Connection(conn.clone()),
            None => TemplateScope::Global,
        };

        self.popup_state = PopupState::SaveTemplate {
            name: String::new(),
            scope,
        };
    }

    pub(super) fn handle_popup_keys(&mut self, key_event: KeyEvent) {
        match &self.popup_state.clone() {
            PopupState::TemplateList { selected } => {
                self.handle_template_list_keys(key_event, *selected);
            }
            PopupState::SaveTemplate { name, scope } => {
                self.handle_save_template_keys(key_event, name.clone(), scope.clone());
            }
            PopupState::ConfirmDelete { index, name } => {
                self.handle_confirm_delete_keys(key_event, *index, name.clone());
            }
            PopupState::None => {}
        }
    }

    fn handle_template_list_keys(&mut self, key_event: KeyEvent, selected: usize) {
        match key_event.code {
            KeyCode::Esc => {
                self.popup_state = PopupState::None;
            }
            KeyCode::Enter => {
                self.apply_selected_template(selected);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let max = self.template_list_cache.len().saturating_sub(1);
                let new_selected = (selected + 1).min(max);
                self.popup_state = PopupState::TemplateList {
                    selected: new_selected,
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let new_selected = selected.saturating_sub(1);
                self.popup_state = PopupState::TemplateList {
                    selected: new_selected,
                };
            }
            KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(template) = self.template_list_cache.get(selected) {
                    self.popup_state = PopupState::ConfirmDelete {
                        index: selected,
                        name: template.name.clone(),
                    };
                }
            }
            KeyCode::Char('g') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.edit_template_in_editor(selected);
            }
            _ => {}
        }
    }

    fn handle_save_template_keys(
        &mut self,
        key_event: KeyEvent,
        mut name: String,
        mut scope: TemplateScope,
    ) {
        match key_event.code {
            KeyCode::Esc => {
                self.popup_state = PopupState::None;
            }
            KeyCode::Enter => {
                if !name.trim().is_empty() {
                    self.save_current_template(name.trim().to_string(), scope);
                    self.popup_state = PopupState::None;
                }
            }
            KeyCode::Tab => {
                // Toggle between global and connection-specific
                scope = match scope {
                    TemplateScope::Global => match &self.current_tab().connected_db {
                        Some(conn) => TemplateScope::Connection(conn.clone()),
                        None => TemplateScope::Global,
                    },
                    TemplateScope::Connection(_) => TemplateScope::Global,
                };
                self.popup_state = PopupState::SaveTemplate { name, scope };
            }
            KeyCode::Char(c) => {
                name.push(c);
                self.popup_state = PopupState::SaveTemplate { name, scope };
            }
            KeyCode::Backspace => {
                name.pop();
                self.popup_state = PopupState::SaveTemplate { name, scope };
            }
            _ => {}
        }
    }

    fn handle_confirm_delete_keys(&mut self, key_event: KeyEvent, index: usize, _name: String) {
        match key_event.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                // Find the actual index in template_store
                if let Some(template) = self.template_list_cache.get(index) {
                    // Find and delete from store
                    let template_name = template.name.clone();
                    if let Some(store_idx) = self
                        .template_store
                        .templates
                        .iter()
                        .position(|t| t.name == template_name)
                    {
                        self.template_store.delete_template(store_idx);
                        let _ = self.template_store.save();
                        self.current_tab_mut().status_message =
                            Some(format!("Deleted template '{}'", template_name));
                    }
                }
                // Refresh and go back to list
                self.open_template_popup();
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                // Go back to template list
                self.popup_state = PopupState::TemplateList { selected: index };
            }
            _ => {}
        }
    }

    fn save_current_template(&mut self, name: String, scope: TemplateScope) {
        let query: String = self.query_textarea.lines().join("\n");

        let template = Template {
            name: name.clone(),
            query,
            scope,
        };

        self.template_store.add_template(template);
        if let Err(e) = self.template_store.save() {
            self.current_tab_mut().status_message = Some(format!("Failed to save template: {}", e));
        } else {
            self.current_tab_mut().status_message = Some(format!("Saved template '{}'", name));
        }
    }

    fn apply_selected_template(&mut self, selected: usize) {
        let template = match self.template_list_cache.get(selected) {
            Some(t) => t.clone(),
            None => return,
        };

        // Insert query into text area
        self.query_textarea.select_all();
        self.query_textarea.cut();
        self.query_textarea.insert_str(&template.query);

        // Position cursor at end of first <placeholder>
        if let Some((line, col, len)) = crate::templates::find_placeholder(&template.query) {
            // Move to start
            self.query_textarea.move_cursor(CursorMove::Top);
            self.query_textarea.move_cursor(CursorMove::Head);

            // Move to target line
            for _ in 0..line {
                self.query_textarea.move_cursor(CursorMove::Down);
            }

            // Move to end of placeholder (col + len)
            for _ in 0..(col + len) {
                self.query_textarea.move_cursor(CursorMove::Forward);
            }
        }

        self.popup_state = PopupState::None;
        self.current_tab_mut().status_message =
            Some(format!("Inserted template '{}'", template.name));
    }

    pub(super) fn edit_query_in_editor(&mut self) {
        let current_query: String = self.query_textarea.lines().join("\n");

        match crate::editor::edit_in_external_editor(&current_query, "sql") {
            Ok(edited) => {
                self.query_textarea.select_all();
                self.query_textarea.cut();
                self.query_textarea.insert_str(edited.trim_end());
                self.current_tab_mut().status_message =
                    Some("Query updated from editor".to_string());
            }
            Err(e) => {
                self.current_tab_mut().status_message = Some(format!("Editor error: {}", e));
            }
        }
        self.needs_redraw = true;
    }

    fn edit_template_in_editor(&mut self, selected: usize) {
        let template = match self.template_list_cache.get(selected) {
            Some(t) => t.clone(),
            None => return,
        };

        // Serialize full template (name, scope, query) for editing
        let content = TemplateStore::serialize_one(&template);

        match crate::editor::edit_in_external_editor(&content, "sql") {
            Ok(edited) => {
                // Parse the edited content back
                if let Some(new_template) = TemplateStore::parse_one(&edited) {
                    // Find and update the template in the store
                    if let Some(store_template) = self
                        .template_store
                        .templates
                        .iter_mut()
                        .find(|t| t.name == template.name)
                    {
                        store_template.name = new_template.name.clone();
                        store_template.scope = new_template.scope;
                        store_template.query = new_template.query;
                        if let Err(e) = self.template_store.save() {
                            self.current_tab_mut().status_message =
                                Some(format!("Failed to save template: {}", e));
                        } else {
                            self.current_tab_mut().status_message =
                                Some(format!("Updated template '{}'", new_template.name));
                        }
                    }
                } else {
                    self.current_tab_mut().status_message =
                        Some("Invalid template format".to_string());
                }

                // Refresh the cache
                self.open_template_popup();
                // Restore selection
                self.popup_state = PopupState::TemplateList { selected };
            }
            Err(e) => {
                self.current_tab_mut().status_message = Some(format!("Editor error: {}", e));
            }
        }
        self.needs_redraw = true;
    }
}
