use super::{Controller, PopupState};
use crate::templates::{Template, TemplateScope, TemplateStore};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_textarea::CursorMove;

impl Controller {
    fn current_connection_name(&self) -> Option<&str> {
        let tab = self.current_tab();
        tab.connections
            .get(tab.connected_index)
            .map(|c| c.name.as_str())
    }

    pub(super) fn open_template_popup(&mut self) {
        let conn_name = self.current_connection_name().unwrap_or("");

        self.template_list_cache = self
            .template_store
            .get_templates_for_connection(conn_name)
            .into_iter()
            .cloned()
            .collect();

        if self.template_list_cache.is_empty() {
            self.popup_state = PopupState::None;
            self.current_tab_mut().status_message =
                Some("No templates saved. Use Ctrl+S to save a template.".to_string());
            return;
        }

        self.popup_state = PopupState::TemplateList {
            selected: 0,
            filter: String::new(),
            searching: false,
        };
    }

    pub(super) fn open_save_template_popup(&mut self) {
        let query: String = self.current_tab().query_textarea.lines().join("\n");
        if query.trim().is_empty() {
            self.current_tab_mut().status_message =
                Some("Cannot save empty query as template".to_string());
            return;
        }

        // Default to current connection
        let connections = self.current_connection_name().unwrap_or("").to_string();

        self.popup_state = PopupState::SaveTemplate {
            name: String::new(),
            connections,
            editing_connections: false,
        };
    }

    pub(super) fn handle_popup_keys(&mut self, key_event: KeyEvent) {
        match &self.popup_state.clone() {
            PopupState::TemplateList {
                selected,
                filter,
                searching,
            } => {
                self.handle_template_list_keys(key_event, *selected, filter.clone(), *searching);
            }
            PopupState::SaveTemplate {
                name,
                connections,
                editing_connections,
            } => {
                self.handle_save_template_keys(
                    key_event,
                    name.clone(),
                    connections.clone(),
                    *editing_connections,
                );
            }
            PopupState::ConfirmDelete {
                index,
                name,
                filter,
            } => {
                self.handle_confirm_delete_keys(key_event, *index, name.clone(), filter.clone());
            }
            PopupState::RecordDetail { .. } => {
                // Handled in handle_output_keys
            }
            PopupState::Completion {
                suggestions,
                selected,
                word_start,
            } => {
                self.handle_completion_keys(key_event, suggestions.clone(), *selected, *word_start);
            }
            PopupState::None => {}
        }
    }

    /// Get templates filtered by the current search filter
    fn filtered_templates(&self, filter: &str) -> Vec<&Template> {
        let filter_lower = filter.to_lowercase();
        self.template_list_cache
            .iter()
            .filter(|t| t.name.to_lowercase().contains(&filter_lower))
            .collect()
    }

    fn handle_template_list_keys(
        &mut self,
        key_event: KeyEvent,
        selected: usize,
        mut filter: String,
        searching: bool,
    ) {
        if searching {
            // Search mode: typing in the filter
            match key_event.code {
                KeyCode::Esc => {
                    // Exit search mode and clear filter
                    self.popup_state = PopupState::TemplateList {
                        selected: 0,
                        filter: String::new(),
                        searching: false,
                    };
                }
                KeyCode::Enter => {
                    // Exit search mode, keep filter
                    self.popup_state = PopupState::TemplateList {
                        selected,
                        filter,
                        searching: false,
                    };
                }
                KeyCode::Char('u') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Clear filter (like vim Ctrl+U)
                    self.popup_state = PopupState::TemplateList {
                        selected: 0,
                        filter: String::new(),
                        searching: true,
                    };
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    self.popup_state = PopupState::TemplateList {
                        selected: 0, // Reset selection when filter changes
                        filter,
                        searching: true,
                    };
                }
                KeyCode::Backspace => {
                    filter.pop();
                    self.popup_state = PopupState::TemplateList {
                        selected: 0, // Reset selection when filter changes
                        filter,
                        searching: true,
                    };
                }
                _ => {}
            }
        } else {
            // Normal mode: navigation and actions
            let filtered = self.filtered_templates(&filter);
            let max = filtered.len().saturating_sub(1);

            match key_event.code {
                KeyCode::Esc => {
                    self.popup_state = PopupState::None;
                }
                KeyCode::Enter => {
                    // Apply the selected template from filtered list
                    if let Some(template) = filtered.get(selected) {
                        // Find index in original cache
                        if let Some(idx) = self
                            .template_list_cache
                            .iter()
                            .position(|t| t.name == template.name)
                        {
                            self.apply_selected_template(idx);
                        }
                    }
                }
                KeyCode::Char('/') => {
                    // Enter search mode
                    self.popup_state = PopupState::TemplateList {
                        selected,
                        filter,
                        searching: true,
                    };
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    let new_selected = (selected + 1).min(max);
                    self.popup_state = PopupState::TemplateList {
                        selected: new_selected,
                        filter,
                        searching: false,
                    };
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    let new_selected = selected.saturating_sub(1);
                    self.popup_state = PopupState::TemplateList {
                        selected: new_selected,
                        filter,
                        searching: false,
                    };
                }
                KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Delete from filtered list
                    if let Some(template) = filtered.get(selected) {
                        self.popup_state = PopupState::ConfirmDelete {
                            index: selected,
                            name: template.name.clone(),
                            filter: filter.clone(),
                        };
                    }
                }
                KeyCode::Char('g') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Edit from filtered list
                    if let Some(template) = filtered.get(selected) {
                        if let Some(idx) = self
                            .template_list_cache
                            .iter()
                            .position(|t| t.name == template.name)
                        {
                            self.edit_template_in_editor(idx);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_save_template_keys(
        &mut self,
        key_event: KeyEvent,
        mut name: String,
        mut connections: String,
        mut editing_connections: bool,
    ) {
        match key_event.code {
            KeyCode::Esc => {
                self.popup_state = PopupState::None;
                return;
            }
            KeyCode::Enter => {
                if !name.trim().is_empty() {
                    let conns: Vec<String> = connections
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let scope = if conns.is_empty() {
                        TemplateScope::Global
                    } else {
                        TemplateScope::Connections(conns)
                    };
                    self.save_current_template(name.trim().to_string(), scope);
                    self.popup_state = PopupState::None;
                }
                return;
            }
            KeyCode::Tab | KeyCode::Up | KeyCode::Down | KeyCode::BackTab => {
                // Switch focus between name and connections
                editing_connections = !editing_connections;
            }
            KeyCode::Char(c) => {
                if editing_connections {
                    connections.push(c);
                } else {
                    name.push(c);
                }
            }
            KeyCode::Backspace => {
                if editing_connections {
                    connections.pop();
                } else {
                    name.pop();
                }
            }
            _ => {}
        }
        self.popup_state = PopupState::SaveTemplate {
            name,
            connections,
            editing_connections,
        };
    }

    fn handle_confirm_delete_keys(
        &mut self,
        key_event: KeyEvent,
        index: usize,
        name: String,
        filter: String,
    ) {
        match key_event.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                // Delete by name (safer than index since list may have changed)
                if let Some(store_idx) = self
                    .template_store
                    .templates
                    .iter()
                    .position(|t| t.name == name)
                {
                    self.template_store.delete_template(store_idx);
                    let _ = self.template_store.save();
                    self.current_tab_mut().status_message =
                        Some(format!("Deleted template '{}'", name));
                }
                // Refresh and go back to list
                self.open_template_popup();
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                // Go back to template list, restoring filter
                // Clamp index to valid range after potential list changes
                let filtered = self.filtered_templates(&filter);
                let new_index = index.min(filtered.len().saturating_sub(1));
                self.popup_state = PopupState::TemplateList {
                    selected: new_index,
                    filter,
                    searching: false,
                };
            }
            _ => {}
        }
    }

    fn save_current_template(&mut self, name: String, scope: TemplateScope) {
        let query: String = self.current_tab().query_textarea.lines().join("\n");

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
        let tab = self.current_tab_mut();
        tab.query_textarea.select_all();
        tab.query_textarea.cut();
        tab.query_textarea.insert_str(&template.query);

        // Position cursor at end of first <placeholder>
        if let Some((line, col, len)) = crate::templates::find_placeholder(&template.query) {
            // Move to start
            tab.query_textarea.move_cursor(CursorMove::Top);
            tab.query_textarea.move_cursor(CursorMove::Head);

            // Move to target line
            for _ in 0..line {
                tab.query_textarea.move_cursor(CursorMove::Down);
            }

            // Move to end of placeholder (col + len)
            for _ in 0..(col + len) {
                tab.query_textarea.move_cursor(CursorMove::Forward);
            }
        }

        self.popup_state = PopupState::None;
        self.current_tab_mut().status_message =
            Some(format!("Inserted template '{}'", template.name));
    }

    pub(super) fn edit_query_in_editor(&mut self) {
        let current_query: String = self.current_tab().query_textarea.lines().join("\n");

        match crate::editor::edit_in_external_editor(&current_query, "sql") {
            Ok(edited) => {
                let tab = self.current_tab_mut();
                tab.query_textarea.select_all();
                tab.query_textarea.cut();
                tab.query_textarea.insert_str(edited.trim_end());
                tab.status_message = Some("Query updated from editor".to_string());
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
                self.popup_state = PopupState::TemplateList {
                    selected,
                    filter: String::new(),
                    searching: false,
                };
            }
            Err(e) => {
                self.current_tab_mut().status_message = Some(format!("Editor error: {}", e));
            }
        }
        self.needs_redraw = true;
    }
}
