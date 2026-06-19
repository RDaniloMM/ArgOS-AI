use iced::widget::{
    button, column, container, row, scrollable, stack, svg, text, text_editor, text_input,
};
use iced::{Background, Border, Color, Element, Fill, Font, Padding, Shadow};

use super::{Message, WorkspaceApp};
use crate::backend;
use crate::workspace::state::{ActiveDocument, ChatEntry, ChatRole, OutputEntry, WorkspaceSection};

const ACCENT: Color = Color::from_rgb(0.31, 0.56, 0.94);
const BG_DARK: Color = Color::from_rgb(0.04, 0.06, 0.12);
const SURFACE: Color = Color::from_rgb(0.08, 0.10, 0.16);
const SURFACE_ALT: Color = Color::from_rgb(0.11, 0.13, 0.19);
const MUTED: Color = Color::from_rgb(0.46, 0.52, 0.63);
const WHITE: Color = Color::from_rgb(1.0, 1.0, 1.0);
const GREEN: Color = Color::from_rgb(0.22, 0.78, 0.44);
const AMBER: Color = Color::from_rgb(0.96, 0.65, 0.14);
const ROSE: Color = Color::from_rgb(0.89, 0.24, 0.35);
const BLUE_LIGHT: Color = Color::from_rgb(0.50, 0.68, 0.94);
const BRAND_OPENAI: Color = Color::from_rgb(0.063, 0.639, 0.498);
const BRAND_ANTHROPIC: Color = Color::from_rgb(0.851, 0.467, 0.024);
const BRAND_GEMINI: Color = Color::from_rgb(0.259, 0.522, 0.957);
const BRAND_DEEPSEEK: Color = Color::from_rgb(0.388, 0.404, 0.945);
const BRAND_OPENCODE: Color = Color::from_rgb(0.024, 0.714, 0.831);
const BRAND_OLLAMA: Color = Color::from_rgb(0.545, 0.361, 0.965);
const BRAND_CUSTOM: Color = Color::from_rgb(0.420, 0.447, 0.502);

const UI_FILES: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M6 2h9l5 5v15H6zM14 3.5V8h4.5\"/></svg>"##;
const UI_WORKFLOWS: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M7 4h10v4H7zm-3 7h16v4H4zm3 7h10v2H7z\"/></svg>"##;
const UI_PROVIDER: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M16 1a4 4 0 0 0-4 4v2H7a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-1V5a2 2 0 0 1 4 0v1h2V5a4 4 0 0 0-4-4zM7 9h10v9H7z\"/></svg>"##;
const UI_SETTINGS: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm9-1.7V10.7l-2.4-.5a7.6 7.6 0 0 0-.5-1.3l1.4-2-1.4-1.4-2 1.4a7.6 7.6 0 0 0-1.3-.5l-.5-2.4H10.7l-.5 2.4a7.6 7.6 0 0 0-1.3.5l-2-1.4-1.4 1.4 1.4 2a7.6 7.6 0 0 0-.5 1.3l-2.4.5v2.6l2.4.5c.1.5.3.9.5 1.3l-1.4 2 1.4 1.4 2-1.4c.4.2.8.4 1.3.5l.5 2.4h2.6l.5-2.4c.5-.1.9-.3 1.3-.5l2 1.4 1.4-1.4-1.4-2c.2-.4.4-.8.5-1.3l2.4-.5z\"/></svg>"##;
const UI_CHAT: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M4 4h16v11H7l-3 3z\"/></svg>"##;
const UI_PLAY: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M8 5v14l11-7z\"/></svg>"##;
const UI_SAVE: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d=\"M5 3h11l3 3v15H5zm3 0v5h8V3\"/></svg>"##;

fn ui_icon(icon: &'static [u8]) -> iced::widget::svg::Handle {
    iced::widget::svg::Handle::from_memory(icon)
}

fn provider_brand(icon_id: &str) -> (&'static str, Color) {
    match icon_id {
        "openai" => ("O", BRAND_OPENAI),
        "anthropic" => ("A", BRAND_ANTHROPIC),
        "google" => ("G", BRAND_GEMINI),
        "deepseek" => ("D", BRAND_DEEPSEEK),
        "opencode" => ("OC", BRAND_OPENCODE),
        "ollama" => ("O", BRAND_OLLAMA),
        _ => ("C", BRAND_CUSTOM),
    }
}

fn provider_svg_handle(icon_id: &str) -> Option<iced::widget::svg::Handle> {
    match icon_id {
        "openai" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/openai.svg"
        ))),
        "anthropic" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/anthropic.svg"
        ))),
        "google" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/gemini.svg"
        ))),
        "deepseek" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/deepseek.svg"
        ))),
        "ollama" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/ollama.svg"
        ))),
        "openrouter" => Some(iced::widget::svg::Handle::from_memory(include_bytes!(
            "../../assets/icons/openrouter.svg"
        ))),
        "opencode" | "custom" => None,
        _ => None,
    }
}

fn surface_card(radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(SURFACE_ALT)),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    }
}

fn subtle_card(radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.04))),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            width: 1.0,
            radius: radius.into(),
        },
        ..Default::default()
    }
}

impl WorkspaceApp {
    pub fn view(&self) -> Element<'_, Message> {
        let body = row![
            self.sidebar(),
            column![self.workspace_shell(), self.output_panel()]
                .width(Fill)
                .height(Fill)
                .spacing(12),
        ]
        .width(Fill)
        .height(Fill)
        .padding(Padding::from(12))
        .spacing(12);

        let base: Element<Message> = container(body)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(BG_DARK)),
                ..Default::default()
            })
            .into();

        if self.modal_open {
            stack([base, self.provider_modal()]).into()
        } else {
            base
        }
    }

    fn workspace_shell(&self) -> Element<'_, Message> {
        row![self.center_pane(), self.chat_pane()]
            .spacing(12)
            .height(Fill)
            .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let header = row![
            container(svg(ui_icon(UI_CHAT)).width(22).height(22))
                .center_x(Fill)
                .center_y(Fill)
                .width(40)
                .height(40)
                .style(|_| container::Style {
                    background: Some(Background::Color(ACCENT)),
                    border: Border {
                        radius: 12.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            column![
                text("ArgOS").size(15).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                text("Workspace").size(11).color(MUTED)
            ]
            .spacing(2)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);

        let nav = column(WorkspaceSection::ALL.into_iter().map(|section| {
            let active = self.active_section == section;
            button(
                row![
                    container(
                        svg(ui_icon(self.section_icon(section)))
                            .width(16)
                            .height(16)
                    )
                    .center_x(Fill)
                    .center_y(Fill)
                    .width(28)
                    .height(28)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(if active {
                            Color::from_rgba(1.0, 1.0, 1.0, 0.15)
                        } else {
                            Color::from_rgba(1.0, 1.0, 1.0, 0.05)
                        })),
                        border: Border {
                            radius: 8.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    text(section.label())
                        .size(13)
                        .color(if active { WHITE } else { MUTED })
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
                .padding(Padding::from([8, 10])),
            )
            .width(Fill)
            .style(move |_, _| {
                if active {
                    button::Style {
                        background: Some(Background::Color(Color::from_rgba(
                            ACCENT.r, ACCENT.g, ACCENT.b, 0.12,
                        ))),
                        border: Border {
                            color: Color::from_rgba(ACCENT.r, ACCENT.g, ACCENT.b, 0.25),
                            width: 1.0,
                            radius: 12.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    button::Style::default()
                }
            })
            .on_press(Message::SectionSelected(section))
            .into()
        }))
        .spacing(8);

        let content = match self.active_section {
            WorkspaceSection::Explorer => self.file_list(),
            WorkspaceSection::Workflows => self.workflow_list(),
            WorkspaceSection::Provider => self.provider_sidebar_summary(),
            WorkspaceSection::Settings => self.settings_sidebar_summary(),
        };

        container(column![header, nav, content].spacing(14))
            .width(260)
            .height(Fill)
            .padding(Padding::from(18))
            .style(|_| surface_card(18.0))
            .into()
    }

    fn section_icon(&self, section: WorkspaceSection) -> &'static [u8] {
        match section {
            WorkspaceSection::Explorer => UI_FILES,
            WorkspaceSection::Workflows => UI_WORKFLOWS,
            WorkspaceSection::Provider => UI_PROVIDER,
            WorkspaceSection::Settings => UI_SETTINGS,
        }
    }

    fn file_list(&self) -> Element<'_, Message> {
        let items: Vec<Element<'_, Message>> = if self.files.is_empty() {
            vec![
                container(text("No curated files were found.").size(12).color(MUTED))
                    .padding(Padding::from(12))
                    .style(|_| subtle_card(10.0))
                    .into(),
            ]
        } else {
            self.files
                .iter()
                .map(|file| {
                    let selected = self.active_document.path() == Some(file.absolute_path.clone());
                    button(
                        column![
                            text(&file.title).size(13).color(WHITE),
                            text(&file.relative_path).size(11).color(MUTED)
                        ]
                        .spacing(4),
                    )
                    .width(Fill)
                    .padding(Padding::from(12))
                    .style(move |_, _| {
                        if selected {
                            button::Style {
                                background: Some(Background::Color(Color::from_rgba(
                                    ACCENT.r, ACCENT.g, ACCENT.b, 0.12,
                                ))),
                                border: Border {
                                    color: Color::from_rgba(ACCENT.r, ACCENT.g, ACCENT.b, 0.25),
                                    width: 1.0,
                                    radius: 12.0.into(),
                                },
                                ..Default::default()
                            }
                        } else {
                            button::Style::default()
                        }
                    })
                    .on_press(Message::FileSelected(file.absolute_path.clone()))
                    .into()
                })
                .collect()
        };

        column![
            text("Explorer").size(12).color(BLUE_LIGHT),
            scrollable(column(items).spacing(8)).height(Fill)
        ]
        .spacing(10)
        .into()
    }

    fn workflow_list(&self) -> Element<'_, Message> {
        let header = column![
            text("n8n Workflows").size(12).color(BLUE_LIGHT),
            text(&self.n8n_message).size(11).color(MUTED)
        ]
        .spacing(4);

        let items: Vec<Element<'_, Message>> = if self.workflows.is_empty() {
            vec![
                container(text("No workflows available.").size(12).color(MUTED))
                    .padding(Padding::from(12))
                    .style(|_| subtle_card(10.0))
                    .into(),
            ]
        } else {
            self.workflows
                .iter()
                .map(|workflow| {
                    let selected = matches!(
                        &self.active_document,
                        ActiveDocument::Workflow(active) if active.id == workflow.id
                    );
                    button(
                        column![
                            text(&workflow.name).size(13).color(WHITE),
                            text(&workflow.id).size(11).color(MUTED)
                        ]
                        .spacing(4),
                    )
                    .width(Fill)
                    .padding(Padding::from(12))
                    .style(move |_, _| {
                        if selected {
                            button::Style {
                                background: Some(Background::Color(Color::from_rgba(
                                    ACCENT.r, ACCENT.g, ACCENT.b, 0.12,
                                ))),
                                border: Border {
                                    color: Color::from_rgba(ACCENT.r, ACCENT.g, ACCENT.b, 0.25),
                                    width: 1.0,
                                    radius: 12.0.into(),
                                },
                                ..Default::default()
                            }
                        } else {
                            button::Style::default()
                        }
                    })
                    .on_press(Message::WorkflowSelected(workflow.id.clone()))
                    .into()
                })
                .collect()
        };

        column![header, scrollable(column(items).spacing(8)).height(Fill)]
            .spacing(10)
            .into()
    }
    fn provider_sidebar_summary(&self) -> Element<'_, Message> {
        let summary = if let Some(provider) = &self.current_provider {
            column![
                text("Configured provider").size(12).color(BLUE_LIGHT),
                text(&provider.preset_id).size(14).color(WHITE),
                text(&provider.model).size(11).color(MUTED)
            ]
        } else {
            column![
                text("Configured provider").size(12).color(BLUE_LIGHT),
                text("None").size(14).color(WHITE),
                text("Open the provider view to save one.")
                    .size(11)
                    .color(MUTED)
            ]
        };

        container(summary.spacing(6))
            .padding(Padding::from(14))
            .style(|_| subtle_card(12.0))
            .into()
    }

    fn settings_sidebar_summary(&self) -> Element<'_, Message> {
        container(
            column![
                text("Desktop runtime").size(12).color(BLUE_LIGHT),
                text(format!("Vault: {}", self.vault_backend))
                    .size(13)
                    .color(WHITE),
                text(format!("n8n: {}", self.n8n_mode_label))
                    .size(11)
                    .color(MUTED)
            ]
            .spacing(6),
        )
        .padding(Padding::from(14))
        .style(|_| subtle_card(12.0))
        .into()
    }

    fn center_pane(&self) -> Element<'_, Message> {
        let header = self.document_header();
        let body = match self.active_section {
            WorkspaceSection::Provider => self.provider_view(),
            WorkspaceSection::Settings => self.settings_view(),
            _ => self.editor_view(),
        };

        container(column![header, body].spacing(12))
            .width(Fill)
            .height(Fill)
            .padding(Padding::from(16))
            .style(|_| surface_card(18.0))
            .into()
    }

    fn document_header(&self) -> Element<'_, Message> {
        let title = self.active_document.title();
        let subtitle = self.active_document.subtitle();
        let save_button = button(
            row![
                svg(ui_icon(UI_SAVE)).width(14).height(14),
                text(if self.file_saving {
                    "Saving..."
                } else {
                    "Save"
                })
                .size(12)
                .color(WHITE)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from([8, 12]))
        .style(|_, _| button::Style {
            background: Some(Background::Color(ACCENT)),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press_maybe(if self.active_document.editable() && !self.file_saving {
            Some(Message::SaveActiveFile)
        } else {
            None
        });

        let run_button = button(
            row![
                svg(ui_icon(UI_PLAY)).width(14).height(14),
                text(if self.workflow_running.is_some() {
                    "Running..."
                } else {
                    "Run workflow"
                })
                .size(12)
                .color(WHITE)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from([8, 12]))
        .style(|_, _| button::Style {
            background: Some(Background::Color(GREEN)),
            border: Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press_maybe(match &self.active_document {
            ActiveDocument::Workflow(workflow) if self.workflow_running.is_none() => {
                Some(Message::RunWorkflow(workflow.id.clone()))
            }
            _ => None,
        });

        let trailing: Element<'_, Message> = match self.active_document {
            ActiveDocument::File(_) => save_button.into(),
            ActiveDocument::Workflow(_) => run_button.into(),
            ActiveDocument::Empty => container("").into(),
        };

        row![
            column![
                text(title).size(22).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                text(subtitle).size(12).color(MUTED)
            ]
            .spacing(4)
            .width(Fill),
            trailing
        ]
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn editor_view(&self) -> Element<'_, Message> {
        if self.workspace_loading || self.file_loading {
            return container(text("Loading workspace...").size(14).color(MUTED))
                .center_x(Fill)
                .center_y(Fill)
                .width(Fill)
                .height(Fill)
                .into();
        }

        match &self.active_document {
            ActiveDocument::File(_) => container(
                text_editor(&self.editor)
                    .height(Fill)
                    .on_action(Message::EditorAction),
            )
            .width(Fill)
            .height(Fill)
            .style(|_| surface_card(14.0))
            .padding(Padding::from(8))
            .into(),
            ActiveDocument::Workflow(workflow) => scrollable(
                container(
                    text(&workflow.content)
                        .size(13)
                        .color(Color::from_rgb(0.82, 0.85, 0.91)),
                )
                .padding(Padding::from(18))
                .width(Fill)
                .style(|_| surface_card(14.0)),
            )
            .height(Fill)
            .into(),
            ActiveDocument::Empty => container(
                text("Select a curated file or workflow from the left sidebar.")
                    .size(14)
                    .color(MUTED),
            )
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .into(),
        }
    }

    fn provider_view(&self) -> Element<'_, Message> {
        let intro = column![
            text("Provider").size(12).color(BLUE_LIGHT),
            text("Use your existing provider config to power assistant chat.")
                .size(13)
                .color(MUTED)
        ]
        .spacing(4);

        let cards: Vec<Element<'_, Message>> = self
            .presets
            .iter()
            .enumerate()
            .map(|(idx, preset)| self.provider_card(preset, idx))
            .collect();

        scrollable(column![intro, row(cards).spacing(12)].spacing(14))
            .height(Fill)
            .into()
    }

    fn provider_card<'a>(
        &'a self,
        preset: &'a backend::ProviderPreset,
        idx: usize,
    ) -> Element<'a, Message> {
        let selected = self.selected_preset_idx == Some(idx);
        let (_, brand) = provider_brand(&preset.icon);

        button(
            column![
                self.provider_icon(&preset.icon, 44.0, selected),
                text(&preset.name).size(15).color(WHITE),
                text(&preset.description).size(12).color(MUTED),
                text(&preset.default_model).size(11).color(BLUE_LIGHT)
            ]
            .spacing(8)
            .align_x(iced::Alignment::Start)
            .padding(Padding::from(18)),
        )
        .width(Fill)
        .style(move |_, _| {
            if selected {
                button::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        brand.r, brand.g, brand.b, 0.15,
                    ))),
                    border: Border {
                        color: Color::from_rgba(brand.r, brand.g, brand.b, 0.50),
                        width: 1.0,
                        radius: 14.0.into(),
                    },
                    ..Default::default()
                }
            } else {
                button::Style {
                    background: Some(Background::Color(SURFACE)),
                    border: Border {
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                        width: 1.0,
                        radius: 14.0.into(),
                    },
                    ..Default::default()
                }
            }
        })
        .on_press(Message::OpenPresetForm(idx))
        .into()
    }

    fn provider_icon(&self, icon_id: &str, size: f32, selected: bool) -> Element<'_, Message> {
        let (_, brand) = provider_brand(icon_id);
        let background = if selected {
            brand
        } else {
            Color::from_rgba(brand.r, brand.g, brand.b, 0.25)
        };

        let content: Element<'_, Message> = if let Some(handle) = provider_svg_handle(icon_id) {
            svg(handle).width(size * 0.55).height(size * 0.55).into()
        } else {
            text(provider_brand(icon_id).0)
                .size(14)
                .color(WHITE)
                .font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                })
                .into()
        };

        container(content)
            .center_x(Fill)
            .center_y(Fill)
            .width(size)
            .height(size)
            .style(move |_| container::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        scrollable(
            column![
                self.settings_item("Vault backend", &self.vault_backend),
                self.settings_item("n8n mode", &self.n8n_mode_label),
                self.settings_item("n8n status", &self.n8n_message),
                self.settings_item(
                    "Provider state",
                    if self.current_provider.is_some() {
                        "Configured"
                    } else {
                        "Missing"
                    }
                ),
            ]
            .spacing(10),
        )
        .height(Fill)
        .into()
    }

    fn settings_item<'a>(&self, title: &'a str, value: &'a str) -> Element<'a, Message> {
        container(
            column![
                text(title).size(12).color(BLUE_LIGHT),
                text(value).size(13).color(WHITE)
            ]
            .spacing(6),
        )
        .padding(Padding::from(14))
        .style(|_| subtle_card(12.0))
        .into()
    }
    fn chat_pane(&self) -> Element<'_, Message> {
        let model_label = self
            .current_provider
            .as_ref()
            .map(|provider| provider.model.as_str())
            .unwrap_or("No provider")
            .to_string();

        let header = row![
            text("Assistant").size(16).color(WHITE).font(Font {
                weight: iced::font::Weight::Semibold,
                ..Font::default()
            }),
            container(text(model_label).size(11).color(BLUE_LIGHT))
                .padding(Padding::from([4, 8]))
                .style(|_| subtle_card(10.0))
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let messages: Vec<Element<'_, Message>> = if self.chat_entries.is_empty() {
            vec![container(text("Ask about the selected file, or run a quick draft with your configured provider.").size(12).color(MUTED))
                .padding(Padding::from(12))
                .style(|_| subtle_card(12.0))
                .into()]
        } else {
            self.chat_entries
                .iter()
                .map(|entry| self.chat_entry(entry))
                .collect()
        };

        let input = row![
            text_input("Ask the assistant...", &self.chat_input)
                .on_input(Message::ChatInputChanged)
                .padding(Padding::from(10))
                .size(13)
                .width(Fill),
            button(
                text(if self.assistant_loading {
                    "Sending..."
                } else {
                    "Send"
                })
                .size(12)
                .color(WHITE)
            )
            .padding(Padding::from([10, 14]))
            .style(|_, _| button::Style {
                background: Some(Background::Color(ACCENT)),
                border: Border {
                    radius: 10.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press_maybe(
                if self.assistant_loading || self.chat_input.trim().is_empty() {
                    None
                } else {
                    Some(Message::SendChat)
                }
            )
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        container(
            column![
                header,
                scrollable(column(messages).spacing(10)).height(Fill),
                input
            ]
            .spacing(12),
        )
        .width(360)
        .height(Fill)
        .padding(Padding::from(16))
        .style(|_| surface_card(18.0))
        .into()
    }

    fn chat_entry<'a>(&self, entry: &'a ChatEntry) -> Element<'a, Message> {
        let (label, accent) = match entry.role {
            ChatRole::User => ("You", BLUE_LIGHT),
            ChatRole::Assistant => ("Assistant", GREEN),
            ChatRole::System => ("System", AMBER),
        };
        container(
            column![
                text(label).size(11).color(accent),
                text(&entry.content).size(13).color(WHITE),
                entry
                    .meta
                    .as_ref()
                    .map(|meta| text(meta).size(11).color(MUTED))
                    .unwrap_or_else(|| text("").size(0))
            ]
            .spacing(6),
        )
        .padding(Padding::from(12))
        .style(|_| subtle_card(12.0))
        .into()
    }

    fn output_panel(&self) -> Element<'_, Message> {
        let rows: Vec<Element<'_, Message>> = if self.output.is_empty() {
            vec![container(text("Activity, workflow runs, save operations, and assistant status will appear here.").size(12).color(MUTED))
                .padding(Padding::from(12))
                .style(|_| subtle_card(10.0))
                .into()]
        } else {
            self.output
                .iter()
                .map(|entry| self.output_entry(entry))
                .collect()
        };

        container(
            column![
                text("Output & activity").size(14).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                scrollable(column(rows).spacing(8)).height(180)
            ]
            .spacing(10),
        )
        .width(Fill)
        .padding(Padding::from(16))
        .style(|_| surface_card(18.0))
        .into()
    }

    fn output_entry<'a>(&self, entry: &'a OutputEntry) -> Element<'a, Message> {
        let accent = if entry.is_error { ROSE } else { GREEN };
        container(
            row![
                container("")
                    .width(4)
                    .height(Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(accent)),
                        border: Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                column![
                    text(&entry.title).size(12).color(WHITE),
                    text(&entry.detail).size(11).color(MUTED)
                ]
                .spacing(4)
                .width(Fill)
            ]
            .spacing(10)
            .padding(Padding::from(12))
            .align_y(iced::Alignment::Center),
        )
        .style(|_| subtle_card(10.0))
        .into()
    }

    fn provider_modal(&self) -> Element<'_, Message> {
        let Some(preset) = self.current_preset() else {
            return container("").into();
        };

        let maybe_result: Element<'_, Message> = if let Some(msg) = &self.form_test_result {
            let color = if msg.starts_with("Error") || msg.starts_with("Failed") {
                ROSE
            } else {
                GREEN
            };
            container(text(msg).size(12).color(color))
                .padding(Padding::from(10))
                .style(|_| subtle_card(10.0))
                .into()
        } else {
            container("").into()
        };

        let card = container(
            column![
                row![
                    column![
                        text(format!("Configure {}", preset.name))
                            .size(18)
                            .color(WHITE),
                        text("Save secure credentials to the desktop vault.")
                            .size(12)
                            .color(MUTED)
                    ]
                    .spacing(4)
                    .width(Fill),
                    button(text("×").size(16).color(MUTED))
                        .style(|_, _| button::Style::default())
                        .on_press(Message::CloseModal)
                ]
                .align_y(iced::Alignment::Center),
                self.modal_field("Endpoint", &self.form_endpoint, Message::EndpointChanged),
                self.modal_field("Model", &self.form_model, Message::ModelChanged),
                column![
                    text("API Key").size(12).color(BLUE_LIGHT),
                    text_input("sk-...", &self.form_api_key)
                        .on_input(Message::ApiKeyChanged)
                        .secure(true)
                        .padding(Padding::from(10))
                        .size(13)
                ]
                .spacing(6),
                maybe_result,
                row![
                    button(text("Test connection").size(12).color(WHITE))
                        .padding(Padding::from([8, 12]))
                        .style(|_, _| button::Style {
                            background: Some(Background::Color(Color::from_rgba(
                                1.0, 1.0, 1.0, 0.08
                            ))),
                            border: Border {
                                radius: 10.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .on_press_maybe(if self.form_testing {
                            None
                        } else {
                            Some(Message::TestConnection)
                        }),
                    button(
                        text(if self.form_saving {
                            "Saving..."
                        } else {
                            "Save provider"
                        })
                        .size(12)
                        .color(WHITE)
                    )
                    .padding(Padding::from([8, 12]))
                    .style(|_, _| button::Style {
                        background: Some(Background::Color(ACCENT)),
                        border: Border {
                            radius: 10.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .on_press_maybe(if self.form_saving {
                        None
                    } else {
                        Some(Message::SaveProvider)
                    })
                ]
                .spacing(8)
            ]
            .spacing(14),
        )
        .padding(Padding::from(24))
        .max_width(520)
        .style(|_| surface_card(18.0));

        container(stack([
            container("")
                .width(Fill)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.04, 0.06, 0.12, 0.82))),
                    ..Default::default()
                })
                .into(),
            container(card)
                .center_x(Fill)
                .center_y(Fill)
                .width(Fill)
                .height(Fill)
                .into(),
        ]))
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn modal_field<'a, F>(
        &self,
        label: &'a str,
        value: &'a str,
        on_input: F,
    ) -> Element<'a, Message>
    where
        F: Fn(String) -> Message + 'static + Copy,
    {
        column![
            text(label).size(12).color(BLUE_LIGHT),
            text_input(label, value)
                .on_input(on_input)
                .padding(Padding::from(10))
                .size(13)
        ]
        .spacing(6)
        .into()
    }
}
