//! ArgOS desktop UI — Iced 0.14 native GUI.
//!
//! Replaces the former Tauri v2 + React frontend with a pure-Rust, zero-bridge
//! desktop application. The same backend logic (provider config, vault, presets,
//! connectivity testing) lives in [`backend`] and is called directly.

use iced::{
    widget::{button, column, container, row, scrollable, stack, svg, text, text_input},
    Background, Border, Color, Element, Fill, Font, Length, Padding, Shadow, Task, Theme,
};

mod backend;

// ---------------------------------------------------------------------------
// Theme constants
// ---------------------------------------------------------------------------

const ACCENT: Color = Color::from_rgb(0.31, 0.56, 0.94);
const MUTED: Color = Color::from_rgb(0.46, 0.52, 0.63);
const GREEN: Color = Color::from_rgb(0.22, 0.78, 0.44);
const AMBER: Color = Color::from_rgb(0.96, 0.65, 0.14);
const ROSE: Color = Color::from_rgb(0.89, 0.24, 0.35);
const BG_DARK: Color = Color::from_rgb(0.04, 0.06, 0.12);
const BLUE_LIGHT: Color = Color::from_rgb(0.50, 0.68, 0.94);
const WHITE: Color = Color::from_rgb(1.0, 1.0, 1.0);

// ---------------------------------------------------------------------------
// Provider brand constants
// ---------------------------------------------------------------------------

const BRAND_OPENAI: Color = Color::from_rgb(0.063, 0.639, 0.498);
const BRAND_ANTHROPIC: Color = Color::from_rgb(0.851, 0.467, 0.024);
const BRAND_GEMINI: Color = Color::from_rgb(0.259, 0.522, 0.957);
const BRAND_DEEPSEEK: Color = Color::from_rgb(0.388, 0.404, 0.945);
const BRAND_OPENCODE: Color = Color::from_rgb(0.024, 0.714, 0.831);
const BRAND_OLLAMA: Color = Color::from_rgb(0.545, 0.361, 0.965);
const BRAND_CUSTOM: Color = Color::from_rgb(0.420, 0.447, 0.502);

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

fn provider_svg_handle(icon_id: &str) -> svg::Handle {
    match icon_id {
        "openai" => svg::Handle::from_memory(include_bytes!("../assets/icons/openai.svg")),
        "anthropic" => svg::Handle::from_memory(include_bytes!("../assets/icons/anthropic.svg")),
        "google" => svg::Handle::from_memory(include_bytes!("../assets/icons/gemini.svg")),
        "deepseek" => svg::Handle::from_memory(include_bytes!("../assets/icons/deepseek.svg")),
        "openrouter" => svg::Handle::from_memory(include_bytes!("../assets/icons/openrouter.svg")),
        "ollama" => svg::Handle::from_memory(include_bytes!("../assets/icons/ollama.svg")),
        _ => svg::Handle::from_memory(include_bytes!("../assets/icons/openrouter.svg")),
    }
}

fn ui_icon(svg: &'static [u8]) -> svg::Handle {
    svg::Handle::from_memory(svg)
}

// UI icons — inline SVG, 24×24, white fill.
const UI_OVERVIEW: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M3 3h8v8H3zm0 10h8v8H3zm10-10h8v8h-8zm0 10h8v8h-8z"/></svg>"##;
const UI_PROVIDER: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M16 1a4 4 0 0 0-4 4v2H7a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-1V5a2 2 0 0 1 4 0v1h2V5a4 4 0 0 0-4-4zM7 9h10v9H7z"/></svg>"##;
const UI_WORKFLOWS: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2zm0 18a8 8 0 1 1 8-8 8 8 0 0 1-8 8zm0-14a6 6 0 1 0 6 6 6 6 0 0 0-6-6zm0 10a4 4 0 1 1 4-4 4 4 0 0 1-4 4z"/></svg>"##;
const UI_KNOWLEDGE: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/></svg>"##;
const UI_AGENTS: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 2l2.4 7.2h7.6l-6 4.8 2.4 7.2-6-4.8-6 4.8 2.4-7.2-6-4.8h7.6z"/></svg>"##;
const UI_SETTINGS: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm9-1.7V10.7l-2.4-.5a7.6 7.6 0 0 0-.5-1.3l1.4-2-1.4-1.4-2 1.4a7.6 7.6 0 0 0-1.3-.5l-.5-2.4H10.7l-.5 2.4a7.6 7.6 0 0 0-1.3.5l-2-1.4-1.4 1.4 1.4 2a7.6 7.6 0 0 0-.5 1.3l-2.4.5v2.6l2.4.5c.1.5.3.9.5 1.3l-1.4 2 1.4 1.4 2-1.4c.4.2.8.4 1.3.5l.5 2.4h2.6l.5-2.4c.5-.1.9-.3 1.3-.5l2 1.4 1.4-1.4-1.4-2c.2-.4.4-.8.5-1.3l2.4-.5z"/></svg>"##;
const UI_SYNC: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 4a8 8 0 0 0-8 8h2a6 6 0 0 1 6-6V2l4 4-4 4V6zM4 12a8 8 0 0 0 8 8v-3a6 6 0 0 1-6-6H4zm8 8l4 4v-3a6 6 0 0 0 6-6h-3a8 8 0 0 1-7 5z"/></svg>"##;
const UI_BRAIN: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 2a7 7 0 0 0-7 7c0 2.4 1.2 4.5 3 5.7V20a2 2 0 0 0 2 2h4a2 2 0 0 0 2-2v-5.3c1.8-1.2 3-3.3 3-5.7a7 7 0 0 0-7-7zm0 2a5 5 0 0 1 5 5c0 1.6-.8 3-2 3.9V18h-2v-3h-2v3H9v-5.1A5 5 0 0 1 7 9a5 5 0 0 1 5-5z"/></svg>"##;
const UI_MONITOR: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M4 4h16v12H4zm0 14h16v2H4zm2-10h12v6H6z"/></svg>"##;
const UI_WARNING: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 2L2 22h20L12 2zm0 4l7 14H5l7-14zm-1 6h2v4h-2zm0 6h2v2h-2z"/></svg>"##;
const UI_CHECK: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/></svg>"##;
const UI_ARROW_RIGHT: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#FFFFFF\"><path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/></svg>"##;
const UI_ARROW_RIGHT_MUTED: &[u8] = br##"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#94a3b8\"><path d="M12 4l-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z"/></svg>"##;

// ---------------------------------------------------------------------------
// Design system helpers
// ---------------------------------------------------------------------------

fn surface_card(radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.11, 0.13, 0.19, 0.60))),
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

fn icon_badge<'a>(icon: &'static [u8], size: f32, accent: Color) -> Element<'a, Message> {
    container(
        svg(ui_icon(icon))
            .width(Length::Fixed(size * 0.55))
            .height(Length::Fixed(size * 0.55)),
    )
    .center_x(Fill)
    .center_y(Fill)
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgba(
            accent.r, accent.g, accent.b, 0.12,
        ))),
        border: Border {
            color: Color::from_rgba(accent.r, accent.g, accent.b, 0.25),
            width: 1.0,
            radius: (size / 2.5).into(),
        },
        ..Default::default()
    })
    .into()
}

fn badge<'a>(label: &'a str, color: Color) -> Element<'a, Message> {
    container(
        text(label)
            .size(11)
            .color(color)
            .font(Font {
                weight: iced::font::Weight::Semibold,
                ..Font::default()
            }),
    )
    .padding(Padding::from([4, 8]))
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(Color::from_rgba(
            color.r, color.g, color.b, 0.12,
        ))),
        border: Border {
            color: Color::from_rgba(color.r, color.g, color.b, 0.25),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Overview,
    Provider,
    Workflows,
    Knowledge,
    Agents,
    Settings,
}

impl Menu {
    const ALL: &'static [Menu] = &[
        Menu::Overview,
        Menu::Provider,
        Menu::Workflows,
        Menu::Knowledge,
        Menu::Agents,
        Menu::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Menu::Overview => "Overview",
            Menu::Provider => "Connect Provider",
            Menu::Workflows => "Workflows",
            Menu::Knowledge => "Knowledge Base",
            Menu::Agents => "Agents",
            Menu::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Menu::Overview => UI_OVERVIEW,
            Menu::Provider => UI_PROVIDER,
            Menu::Workflows => UI_WORKFLOWS,
            Menu::Knowledge => UI_KNOWLEDGE,
            Menu::Agents => UI_AGENTS,
            Menu::Settings => UI_SETTINGS,
        }
    }
}

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Toast {
    id: u64,
    kind: ToastKind,
    title: String,
    message: String,
}

#[derive(Debug, Clone)]
enum ToastKind {
    Success,
    Error,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct ArgosApp {
    active_menu: Menu,
    presets: Vec<backend::ProviderPreset>,
    current_input: Option<backend::ProviderInput>,
    selected_preset_idx: Option<usize>,
    modal_open: bool,
    toasts: Vec<Toast>,
    next_toast_id: u64,

    // Form fields
    form_endpoint: String,
    form_model: String,
    form_api_key: String,
    form_testing: bool,
    form_saving: bool,
    form_test_result: Option<String>,

    load_error: Option<String>,
    loading: bool,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    MenuSelected(Menu),
    NavigateTo(Menu),
    OpenPresetForm(usize),
    CloseModal,
    EndpointChanged(String),
    ModelChanged(String),
    ApiKeyChanged(String),
    TestConnection,
    SaveProvider,
    ConnectionTested(Result<backend::ProviderStatus, String>),
    ProviderSaved(Result<(backend::ProviderInput, String), String>),
    InitialDataLoaded(
        Result<(Vec<backend::ProviderPreset>, Option<backend::ProviderInput>), String>,
    ),
    DismissToast(u64),
}

// ---------------------------------------------------------------------------
// App implementation
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn new() -> (Self, Task<Message>) {
        let app = ArgosApp {
            active_menu: Menu::Overview,
            presets: backend::provider_presets(),
            current_input: None,
            selected_preset_idx: None,
            modal_open: false,
            toasts: Vec::new(),
            next_toast_id: 1,
            form_endpoint: String::new(),
            form_model: String::new(),
            form_api_key: String::new(),
            form_testing: false,
            form_saving: false,
            form_test_result: None,
            load_error: None,
            loading: true,
        };
        let task = Task::perform(Self::load_initial_data(), Message::InitialDataLoaded);
        (app, task)
    }

    async fn load_initial_data(
    ) -> Result<(Vec<backend::ProviderPreset>, Option<backend::ProviderInput>), String>
    {
        let dir = backend::argos_dir()?;
        let vault = argos_security::MemoryVault::new();
        let presets = backend::provider_presets();
        let current = backend::get_current_provider(&dir, &vault).await?;
        Ok((presets, current))
    }

    fn add_toast(&mut self, kind: ToastKind, title: impl Into<String>, message: impl Into<String>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            title: title.into(),
            message: message.into(),
        });
    }

    fn current_preset(&self) -> Option<&backend::ProviderPreset> {
        self.selected_preset_idx
            .and_then(|i| self.presets.get(i))
    }

    fn saved_preset(&self) -> Option<&backend::ProviderPreset> {
        self.current_input
            .as_ref()
            .and_then(|input| self.presets.iter().find(|p| p.id == input.preset_id))
    }

    fn theme(&self) -> Theme {
        Theme::TokyoNight
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::MenuSelected(menu) | Message::NavigateTo(menu) => {
                self.active_menu = menu;
                Task::none()
            }

            Message::OpenPresetForm(idx) => {
                self.selected_preset_idx = Some(idx);
                if let Some(preset) = self.presets.get(idx) {
                    if let Some(ref input) = self.current_input {
                        if input.preset_id == preset.id {
                            self.form_endpoint.clone_from(&input.endpoint);
                            self.form_model.clone_from(&input.model);
                            self.form_api_key.clone_from(&input.api_key);
                        } else {
                            self.form_endpoint = preset.default_endpoint.clone();
                            self.form_model = preset.default_model.clone();
                            self.form_api_key.clear();
                        }
                    } else {
                        self.form_endpoint = preset.default_endpoint.clone();
                        self.form_model = preset.default_model.clone();
                        self.form_api_key.clear();
                    }
                    self.form_test_result = None;
                }
                self.modal_open = true;
                Task::none()
            }

            Message::CloseModal => {
                self.modal_open = false;
                self.form_test_result = None;
                Task::none()
            }

            Message::EndpointChanged(val) => {
                self.form_endpoint = val;
                Task::none()
            }

            Message::ModelChanged(val) => {
                self.form_model = val;
                Task::none()
            }

            Message::ApiKeyChanged(val) => {
                self.form_api_key = val;
                Task::none()
            }

            Message::TestConnection => {
                let preset = match self.current_preset() {
                    Some(p) => p.clone(),
                    None => return Task::none(),
                };
                let input = backend::ProviderInput {
                    preset_id: preset.id,
                    api_key: self.form_api_key.clone(),
                    endpoint: self.form_endpoint.clone(),
                    model: self.form_model.clone(),
                };
                self.form_testing = true;
                self.form_test_result = None;
                Task::perform(
                    async move { backend::test_provider(&input).await },
                    Message::ConnectionTested,
                )
            }

            Message::ConnectionTested(result) => {
                self.form_testing = false;
                match result {
                    Ok(status) => {
                        if status.connected {
                            self.form_test_result = Some(status.message.clone());
                            self.add_toast(
                                ToastKind::Success,
                                "Connection successful",
                                &status.message,
                            );
                        } else {
                            self.form_test_result =
                                Some(format!("Failed: {}", status.message));
                            self.add_toast(
                                ToastKind::Error,
                                "Connection failed",
                                &status.message,
                            );
                        }
                    }
                    Err(e) => {
                        self.form_test_result = Some(format!("Error: {e}"));
                        self.add_toast(ToastKind::Error, "Connection error", &e);
                    }
                }
                Task::none()
            }

            Message::SaveProvider => {
                let preset = match self.current_preset() {
                    Some(p) => p.clone(),
                    None => return Task::none(),
                };
                let input = backend::ProviderInput {
                    preset_id: preset.id.clone(),
                    api_key: self.form_api_key.clone(),
                    endpoint: self.form_endpoint.clone(),
                    model: self.form_model.clone(),
                };
                let name = preset.name.clone();
                self.form_saving = true;
                Task::perform(
                    async move {
                        let dir = backend::argos_dir()?;
                        let mut vault = argos_security::MemoryVault::new();
                        backend::save_provider(&dir, &mut vault, &input).await?;
                        Ok((input, name))
                    },
                    Message::ProviderSaved,
                )
            }

            Message::ProviderSaved(result) => {
                self.form_saving = false;
                match result {
                    Ok((input, name)) => {
                        self.current_input = Some(input);
                        self.modal_open = false;
                        self.form_test_result = None;
                        self.active_menu = Menu::Overview;
                        self.add_toast(
                            ToastKind::Success,
                            "Provider saved",
                            format!("{name} configuration saved."),
                        );
                    }
                    Err(e) => {
                        self.add_toast(ToastKind::Error, "Save failed", &e);
                    }
                }
                Task::none()
            }

            Message::InitialDataLoaded(result) => {
                self.loading = false;
                match result {
                    Ok((presets, current)) => {
                        self.presets = presets;
                        self.current_input = current;
                        self.load_error = None;
                        if let Some(ref input) = self.current_input {
                            self.selected_preset_idx =
                                self.presets.iter().position(|p| p.id == input.preset_id);
                        }
                    }
                    Err(e) => {
                        self.load_error = Some(e.clone());
                        self.add_toast(ToastKind::Error, "Failed to load provider data", &e);
                    }
                }
                Task::none()
            }

            Message::DismissToast(id) => {
                self.toasts.retain(|t| t.id != id);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let sidebar = self.sidebar();
        let main_content = self.main_content();

        let base: Element<Message> = container(
            row![sidebar, main_content].width(Fill).height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .into();

        if self.modal_open {
            let modal_overlay = self.modal();
            stack([base, modal_overlay]).into()
        } else {
            base
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn sidebar(&self) -> Element<'_, Message> {
        let logo = row![
            container(
                svg(ui_icon(UI_AGENTS))
                    .width(Length::Fixed(22.0))
                    .height(Length::Fixed(22.0)),
            )
            .center_x(Fill)
            .center_y(Fill)
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.22, 0.33, 0.94))),
                border: Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.22, 0.33, 0.94, 0.40),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..Default::default()
            }),
            column![
                text("ArgOS").size(15).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                text("Desktop Console").size(11).color(MUTED),
            ]
            .spacing(1),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from(16));

        let mut nav = column![logo].spacing(8).width(240).padding(Padding::from(16));

        for menu in Menu::ALL {
            let is_active = self.active_menu == *menu;
            let icon = menu.icon();
            let btn = button(
                row![
                    container(
                        svg(ui_icon(icon))
                            .width(Length::Fixed(18.0))
                            .height(Length::Fixed(18.0)),
                    )
                    .width(Length::Fixed(28.0))
                    .height(Length::Fixed(28.0))
                    .center_x(Fill)
                    .center_y(Fill)
                    .style(move |_t: &Theme| container::Style {
                        background: Some(Background::Color(if is_active {
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
                    text(menu.label())
                        .size(13)
                        .width(Fill)
                        .color(if is_active { WHITE } else { MUTED })
                        .font(Font {
                            weight: if is_active {
                                iced::font::Weight::Semibold
                            } else {
                            iced::font::Weight::Normal
                            },
                            ..Font::default()
                        }),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center)
                .padding(Padding::from(8)),
            )
            .width(Fill)
            .style(move |_t: &Theme, status| {
                let base = if is_active {
                    button::Style {
                        background: Some(Background::Color(Color::from_rgba(
                            0.31, 0.56, 0.94, 0.12,
                        ))),
                        border: Border {
                            color: Color::from_rgba(0.31, 0.56, 0.94, 0.25),
                            width: 1.0,
                            radius: 12.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..Default::default()
                    }
                };
                match status {
                    button::Status::Hovered if !is_active => button::Style {
                        background: Some(Background::Color(Color::from_rgba(
                            1.0, 1.0, 1.0, 0.05,
                        ))),
                        border: Border {
                            color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                            width: 1.0,
                            radius: 12.0.into(),
                        },
                        ..Default::default()
                    },
                    _ => base,
                }
            })
            .on_press(Message::MenuSelected(*menu));

            nav = nav.push(btn);
        }

        let saved = self.saved_preset();
        let provider_brand_color = saved.map(|p| {
            let (_, c) = provider_brand(&p.icon);
            c
        });

        nav = nav.push(container("").height(Fill));
        nav = nav.push(
            button(
                container(
                    row![
                        container({
                            let icon: Element<'_, Message> = if let Some(p) = saved {
                                let h = provider_svg_handle(&p.icon);
                                svg(h)
                                    .width(Length::Fixed(22.0))
                                    .height(Length::Fixed(22.0))
                                    .into()
                            } else {
                                svg(ui_icon(UI_PROVIDER))
                                    .width(Length::Fixed(22.0))
                                    .height(Length::Fixed(22.0))
                                    .into()
                            };
                            icon
                        })
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(Length::Fixed(40.0))
                        .height(Length::Fixed(40.0))
                        .style(move |_t: &Theme| {
                            let bg = if let Some(c) = provider_brand_color {
                                Color::from_rgba(c.r, c.g, c.b, 0.18)
                            } else {
                                Color::from_rgba(0.96, 0.65, 0.14, 0.15)
                            };
                            container::Style {
                                background: Some(Background::Color(bg)),
                                border: Border {
                                    radius: 12.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                        }),
                        column![
                            text(saved.map(|p| p.name.as_str()).unwrap_or("No provider"))
                                .size(13)
                                .color(WHITE)
                                .font(Font {
                                    weight: iced::font::Weight::Semibold,
                                    ..Font::default()
                                }),
                            text(
                                self.current_input
                                    .as_ref()
                                    .map(|i| i.model.as_str())
                                    .unwrap_or("Connect to unlock modules"),
                            )
                            .size(11)
                            .color(MUTED),
                        ]
                        .spacing(2)
                        .width(Fill),
                        svg(ui_icon(UI_ARROW_RIGHT_MUTED))
                            .width(Length::Fixed(16.0))
                            .height(Length::Fixed(16.0)),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
                    .padding(Padding::from(12)),
                )
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.04))),
                    border: Border {
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                        width: 1.0,
                        radius: 14.0.into(),
                    },
                    ..Default::default()
                }),
            )
            .width(Fill)
            .style(|_t: &Theme, _status| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                ..Default::default()
            })
            .on_press(Message::MenuSelected(Menu::Provider)),
        );

        container(nav)
            .height(Fill)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(
                    0.04, 0.05, 0.10, 0.95,
                ))),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.06),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

// ---------------------------------------------------------------------------
// Main content area
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn main_content(&self) -> Element<'_, Message> {
        if self.loading {
            return container(text("Loading...").color(MUTED).size(20))
                .center_x(Fill)
                .center_y(Fill)
                .width(Fill)
                .height(Fill)
                .into();
        }

        let content: Element<Message> = match self.active_menu {
            Menu::Overview => self.view_overview(),
            Menu::Provider => self.view_provider(),
            Menu::Workflows => self.view_workflows(),
            Menu::Knowledge => self.view_knowledge(),
            Menu::Agents => self.view_agents(),
            Menu::Settings => self.view_settings(),
        };

        let with_toasts: Element<Message> = if self.toasts.is_empty() {
            content
        } else {
            let toasts_column: Element<Message> = column(
                self.toasts.iter().map(|t| self.toast_element(t)),
            )
            .spacing(8)
            .padding(Padding::from(16))
            .width(Length::Shrink)
            .into();

            stack([
                content,
                container(toasts_column)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Top)
                    .width(Fill)
                    .height(Fill)
                    .into(),
            ])
            .into()
        };

        container(with_toasts)
            .width(Fill)
            .height(Fill)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(BG_DARK)),
                ..Default::default()
            })
            .into()
    }

    fn section_header(&self, eyebrow: String, title: String, description: String) -> Element<'_, Message> {
        column![
            text(eyebrow.to_uppercase()).size(11).color(BLUE_LIGHT),
            text(title).size(26).color(WHITE),
            text(description).size(13).color(MUTED),
        ]
        .spacing(6)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 24.0,
            left: 0.0,
        })
        .into()
    }

    fn locked_notice(&self) -> Element<'_, Message> {
        let connect_btn = button(
            row![
                text("Connect provider").size(12).color(WHITE),
                svg(ui_icon(UI_ARROW_RIGHT))
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([6, 12])),
        )
        .style(|_t: &Theme, _status| button::Style {
            background: Some(Background::Color(ACCENT)),
            border: Border {
                radius: 8.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press(Message::NavigateTo(Menu::Provider));

        container(
            row![
                icon_badge(UI_WARNING, 36.0, AMBER),
                column![
                    text("Provider required").size(14).color(WHITE).font(Font {
                        weight: iced::font::Weight::Semibold,
                        ..Font::default()
                    }),
                    text("Connect and save a provider first to unlock workflow modules.")
                        .size(12)
                        .color(MUTED),
                ]
                .spacing(2)
                .width(Fill),
                connect_btn,
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from(16)),
        )
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.96, 0.65, 0.14, 0.08))),
            border: Border {
                color: Color::from_rgba(0.96, 0.65, 0.14, 0.20),
                width: 1.0,
                radius: 12.0.into(),
            },
            ..Default::default()
        })
        .into()
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn view_overview(&self) -> Element<'_, Message> {
        let saved = self.saved_preset();
        let connected = saved.is_some();

        let cta_target = if connected { Menu::Workflows } else { Menu::Provider };
        let cta_label = if connected { "Open Workflows" } else { "Connect Provider" };

        let cta_button = button(
            row![
                text(cta_label).size(13).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                svg(ui_icon(UI_ARROW_RIGHT))
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(16.0)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([10, 18])),
        )
        .style(|_t: &Theme, status| {
            let base = button::Style {
                background: Some(Background::Color(ACCENT)),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                    width: 1.0,
                    radius: 24.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.31, 0.56, 0.94, 0.30),
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                },
                ..Default::default()
            };
            match status {
                button::Status::Hovered => button::Style {
                    background: Some(Background::Color(Color::from_rgb(0.40, 0.62, 0.96))),
                    ..base
                },
                _ => base,
            }
        })
        .on_press(Message::NavigateTo(cta_target));

        let hero = container(
            row![
                column![
                    text("CONTROL CENTER").size(11).color(BLUE_LIGHT).font(Font {
                        weight: iced::font::Weight::Semibold,
                        ..Font::default()
                    }),
                    text("Manage providers, workflows, knowledge, and agents from one desktop shell.")
                        .size(22)
                        .color(WHITE),
                    text("The provider setup is now the first step, not the whole app.")
                        .size(13)
                        .color(MUTED),
                ]
                .spacing(8)
                .width(Fill),
                cta_button,
            ]
            .spacing(16)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from(24)),
        )
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.31, 0.56, 0.94, 0.10))),
            border: Border {
                color: Color::from_rgba(0.31, 0.56, 0.94, 0.25),
                width: 1.0,
                radius: 16.0.into(),
            },
            ..Default::default()
        });

        let status_row = row![
            self.status_card(
                "Provider",
                saved.map(|p| p.name.as_str()).unwrap_or("Pending"),
                UI_PROVIDER,
                if connected { GREEN } else { AMBER },
            ),
            self.status_card(
                "Workflow Modules",
                "3 menus",
                UI_WORKFLOWS,
                BLUE_LIGHT,
            ),
            self.status_card("Desktop Mode", "Enabled", UI_MONITOR, GREEN),
        ]
        .spacing(12);

        let steps_card = container(
            column![
                text("Next steps").size(17).color(WHITE).font(Font {
                    weight: iced::font::Weight::Semibold,
                    ..Font::default()
                }),
                self.step_item(1, true, "Connect provider"),
                self.step_item(2, false, "Configure workflow source"),
                self.step_item(3, false, "Build knowledge index"),
                self.step_item(4, false, "Run an agent session"),
            ]
            .spacing(8)
            .padding(Padding::from(24)),
        )
        .style(|_t: &Theme| surface_card(16.0));

        let modules = row![
            self.module_card(
                "Workflows",
                UI_WORKFLOWS,
                "n8n imports and automation intelligence",
                AMBER,
                Menu::Workflows,
            ),
            self.module_card(
                "Knowledge",
                UI_KNOWLEDGE,
                "Bundles, queries, links, and project memory",
                BLUE_LIGHT,
                Menu::Knowledge,
            ),
            self.module_card(
                "Agents",
                UI_AGENTS,
                "Provider-backed tool execution",
                Color::from_rgb(0.76, 0.48, 0.98),
                Menu::Agents,
            ),
        ]
        .spacing(12);

        let content = column![
            self.section_header(
                "Control Center".to_string(),
                "Overview".to_string(),
                "Status and quick actions.".to_string(),
            ),
            hero,
            status_row,
            row![steps_card.width(380), container(modules).width(Fill)].spacing(16),
        ]
        .spacing(16)
        .padding(Padding::from(32))
        .max_width(960);

        scrollable(container(content).center_x(Fill).width(Fill)).into()
    }

    fn status_card<'a>(
        &self,
        label: &'a str,
        value: &'a str,
        icon: &'static [u8],
        accent: Color,
    ) -> Element<'a, Message> {
        container(
            row![
                icon_badge(icon, 42.0, accent),
                column![
                    text(label.to_uppercase()).size(10).color(MUTED),
                    text(value)
                        .size(14)
                        .color(WHITE)
                        .font(Font {
                            weight: iced::font::Weight::Semibold,
                            ..Font::default()
                        }),
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(14)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from(16)),
        )
        .width(Fill)
        .style(|_t: &Theme| surface_card(12.0))
        .into()
    }

    fn step_item<'a>(&self, step: usize, done: bool, title: &'a str) -> Element<'a, Message> {
        let circle: Element<'a, Message> = if done {
            container(
                svg(ui_icon(UI_CHECK))
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0)),
            )
            .center_x(Fill)
            .center_y(Fill)
            .width(26)
            .height(26)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.22, 0.78, 0.44, 0.18))),
                border: Border {
                    radius: 13.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
        } else {
            container(text(step.to_string()).size(11).color(MUTED))
                .center_x(Fill)
                .center_y(Fill)
                .width(26)
                .height(26)
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.06))),
                    border: Border {
                        radius: 13.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
        };

        container(
            row![
                circle,
                text(title).size(13).color(if done { WHITE } else { MUTED }),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from(12)),
        )
        .width(Fill)
        .style(|_t: &Theme| subtle_card(10.0))
        .into()
    }

    fn module_card<'a>(
        &self,
        title: &'a str,
        icon: &'static [u8],
        description: &'a str,
        accent: Color,
        target: Menu,
    ) -> Element<'a, Message> {
        button(
            column![
                icon_badge(icon, 48.0, accent),
                text(title)
                    .size(16)
                    .color(WHITE)
                    .font(Font {
                        weight: iced::font::Weight::Semibold,
                        ..Font::default()
                    }),
                text(description).size(12).color(MUTED),
            ]
            .spacing(12)
            .padding(Padding::from(20))
            .align_x(iced::Alignment::Start),
        )
        .width(Fill)
        .style(move |_t: &Theme, status| {
            let base = button::Style {
                background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.04))),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                    width: 1.0,
                    radius: 16.0.into(),
                },
                ..Default::default()
            };
            match status {
                button::Status::Hovered => button::Style {
                    background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.08))),
                    border: Border {
                        color: Color::from_rgba(accent.r, accent.g, accent.b, 0.40),
                        width: 1.0,
                        radius: 16.0.into(),
                    },
                    ..Default::default()
                },
                _ => base,
            }
        })
        .on_press(Message::NavigateTo(target))
        .into()
    }
}

// ---------------------------------------------------------------------------
// Provider grid
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn view_provider(&self) -> Element<'_, Message> {
        let error_banner: Option<Element<Message>> = self.load_error.as_ref().map(|err| {
            container(
                row![
                    text("\u{26A0}").size(16).color(ROSE),
                    column![
                        text("Failed to load provider data")
                            .size(13)
                            .color(Color::from_rgb(0.98, 0.71, 0.78)),
                        text(err).size(12).color(Color::from_rgba(0.98, 0.71, 0.78, 0.8)),
                    ]
                    .spacing(2)
                    .width(Fill),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Start)
                .padding(Padding::from(16)),
            )
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.89, 0.24, 0.35, 0.10))),
                border: Border {
                    color: Color::from_rgba(0.89, 0.24, 0.35, 0.25),
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            })
            .into()
        });

        let mut content = column![
            self.section_header(
                "Provider".to_string(),
                "Connect the LLM provider that powers ArgOS".to_string(),
                "Choose a provider, test the connection, and save the configuration.".to_string(),
            ),
        ]
        .spacing(8);

        if let Some(banner) = error_banner {
            content = content.push(banner);
        }

        // Grid: 3 cards per row
        for chunk_start in (0..self.presets.len()).step_by(3) {
            let end = (chunk_start + 3).min(self.presets.len());
            let mut cards: Vec<Element<Message>> = Vec::with_capacity(end - chunk_start);
            for i in chunk_start..end {
                cards.push(self.provider_card(&self.presets[i], i));
            }
            content = content.push(row(cards).spacing(12));
        }

        if !self.presets.is_empty() {
            content = content.push(
                text("Select a provider card to open its configuration modal.")
                    .size(12)
                    .color(MUTED),
            );
        }

        scrollable(container(content).padding(Padding::from(32)).max_width(960).center_x(Fill)).into()
    }

    fn provider_card(&self, preset: &backend::ProviderPreset, idx: usize) -> Element<'_, Message> {
        let is_selected = self.selected_preset_idx == Some(idx);
        let (_, brand_color) = provider_brand(&preset.icon);
        let name = preset.name.clone();
        let description = preset.description.clone();
        let default_model = preset.default_model.clone();
        let icon_id = preset.icon.clone();
        let svg_handle = provider_svg_handle(&icon_id);

        button(
            column![
                container(
                    svg(svg_handle)
                        .width(Length::Fixed(24.0))
                        .height(Length::Fixed(24.0))
                )
                    .center_x(Fill)
                    .center_y(Fill)
                    .width(44)
                    .height(44)
                    .style(move |_t: &Theme| container::Style {
                        background: Some(Background::Color(if is_selected {
                            brand_color
                        } else {
                            Color::from_rgba(
                                brand_color.r * 0.3,
                                brand_color.g * 0.3,
                                brand_color.b * 0.3,
                                0.60,
                            )
                        })),
                        border: Border {
                            radius: 10.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                text(name).size(15).color(WHITE),
                text(description).size(12).color(MUTED),
                text(default_model).size(11).color(BLUE_LIGHT),
            ]
            .spacing(8)
            .padding(Padding::from(20))
            .align_x(iced::Alignment::Start),
        )
        .width(Fill)
        .style(move |_t: &Theme, _status| {
            if is_selected {
                button::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        brand_color.r * 0.15,
                        brand_color.g * 0.15,
                        brand_color.b * 0.15,
                        0.25,
                    ))),
                    border: Border {
                        color: Color::from_rgba(brand_color.r, brand_color.g, brand_color.b, 0.60),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            } else {
                button::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        0.08, 0.10, 0.18, 0.80,
                    ))),
                    border: Border {
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                        width: 1.0,
                        radius: 12.0.into(),
                    },
                    ..Default::default()
                }
            }
        })
        .on_press(Message::OpenPresetForm(idx))
        .into()
    }
}

// ---------------------------------------------------------------------------
// Other panels
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn view_workflows(&self) -> Element<'_, Message> {
        let locked = self.current_input.is_none();
        let mut col = column![
            self.section_header(
                "Workflows".to_string(),
                "Automation workspace".to_string(),
                "Prepare n8n imports, compare existing workflows, and keep execution state in a desktop-first surface.".to_string(),
            ),
        ]
        .spacing(8);

        if locked {
            col = col.push(self.locked_notice());
        }

        col = col.push(self.desktop_panel_items(vec![
            (
                "n8n Sync",
                UI_SYNC,
                "Import, inspect, and prepare n8n workflows for agent execution.",
                "Ready for connector setup",
                AMBER,
            ),
            (
                "Workflow Intelligence",
                UI_BRAIN,
                "Find similar automations and suggest reusable steps across your library.",
                if locked { "Provider required" } else { "Available" },
                if locked { BLUE_LIGHT } else { GREEN },
            ),
            (
                "Execution Monitor",
                UI_MONITOR,
                "Track desktop runs, retries, and human approvals from one console.",
                "Coming next",
                MUTED,
            ),
        ]));

        scrollable(container(col).padding(Padding::from(32)).max_width(960).center_x(Fill)).into()
    }

    fn view_knowledge(&self) -> Element<'_, Message> {
        let locked = self.current_input.is_none();
        let mut col = column![
            self.section_header(
                "Knowledge Base".to_string(),
                "Project memory and retrieval".to_string(),
                "The backend already contains knowledge, bundle, parser, query, and link modules.".to_string(),
            ),
        ]
        .spacing(8);

        if locked {
            col = col.push(self.locked_notice());
        }

        col = col.push(self.list_panel_items(vec![
            "Ingest local docs and notes into ArgOS knowledge bundles.",
            "Query saved context before asking the provider.",
            "Lint broken links and keep cross references healthy.",
        ]));

        scrollable(container(col).padding(Padding::from(32)).max_width(960).center_x(Fill)).into()
    }

    fn view_agents(&self) -> Element<'_, Message> {
        let locked = self.current_input.is_none();
        let mut col = column![
            self.section_header(
                "Agents".to_string(),
                "Agent control room".to_string(),
                "Expose the generic agent runtime as a visible desktop module.".to_string(),
            ),
        ]
        .spacing(8);

        if locked {
            col = col.push(self.locked_notice());
        }

        col = col.push(self.list_panel_items(vec![
            "Generic agent runtime",
            "Tool registry and permissions",
            "Provider-backed planning loop",
        ]));

        scrollable(container(col).padding(Padding::from(32)).max_width(960).center_x(Fill)).into()
    }

    fn view_settings(&self) -> Element<'_, Message> {
        let col = column![
            self.section_header(
                "Settings".to_string(),
                "Desktop preferences".to_string(),
                "A central place for vault, storage, provider, and connector settings.".to_string(),
            ),
            self.list_panel_items(vec![
                "Provider credentials are stored through the vault layer.",
                "Config writes target the local .argos config directory.",
                "Additional desktop preferences can live here as backend commands are added.",
            ]),
        ]
        .spacing(8);

        scrollable(container(col).padding(Padding::from(32)).max_width(960).center_x(Fill)).into()
    }

    fn desktop_panel_items<'a>(
        &self,
        items: Vec<(&'a str, &'static [u8], &'a str, &'a str, Color)>,
    ) -> Element<'a, Message> {
        let cards: Vec<Element<Message>> = items
            .into_iter()
            .map(|(title, icon, desc, status, accent)| {
                container(
                    column![
                        icon_badge(icon, 48.0, accent),
                        text(title)
                            .size(17)
                            .color(WHITE)
                            .font(Font {
                                weight: iced::font::Weight::Semibold,
                                ..Font::default()
                            }),
                        text(desc).size(13).color(MUTED),
                        badge(status, accent),
                    ]
                    .spacing(12)
                    .padding(Padding::from(20))
                    .align_x(iced::Alignment::Start),
                )
                .width(Fill)
                .style(|_t: &Theme| surface_card(16.0))
                .into()
            })
            .collect();

        row(cards).spacing(12).into()
    }

    fn list_panel_items<'a>(&self, items: Vec<&'a str>) -> Element<'a, Message> {
        let bullets: Vec<Element<Message>> = items
            .into_iter()
            .map(|item| {
                container(
                    row![
                        container(
                            svg(ui_icon(UI_CHECK))
                                .width(Length::Fixed(14.0))
                                .height(Length::Fixed(14.0)),
                        )
                        .center_x(Fill)
                        .center_y(Fill)
                        .width(24)
                        .height(24)
                        .style(|_t: &Theme| container::Style {
                            background: Some(Background::Color(Color::from_rgba(
                                0.31, 0.56, 0.94, 0.12,
                            ))),
                            border: Border {
                                radius: 12.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        text(item).size(13).color(Color::from_rgb(0.72, 0.76, 0.84)),
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
                    .padding(Padding::from(14)),
                )
                .width(Fill)
                .style(|_t: &Theme| subtle_card(12.0))
                .into()
            })
            .collect();

        column(bullets).spacing(8).into()
    }
}

// ---------------------------------------------------------------------------
// Provider form modal
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn modal(&self) -> Element<'_, Message> {
        let preset = match self.current_preset() {
            Some(p) => p,
            None => return container("").into(),
        };

        let backdrop: Element<Message> = container(
            container("")
                .width(Fill)
                .height(Fill)
                .style(|_t: &Theme| container::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        0.04, 0.06, 0.12, 0.80,
                    ))),
                    ..Default::default()
                }),
        )
        .width(Fill)
        .height(Fill)
        .into();

        let form = self.modal_form(preset);
        stack([backdrop, form]).into()
    }

    fn modal_form(&self, preset: &backend::ProviderPreset) -> Element<'_, Message> {
        let title = format!("Configure {}", preset.name);
        let description = format!("Enter the details for your {} provider.", preset.name);

        let endpoint_input = column![
            text("Endpoint").size(13).color(Color::from_rgb(0.72, 0.76, 0.84)),
            text_input("Base URL", &self.form_endpoint)
                .on_input(Message::EndpointChanged)
                .padding(Padding::from(10))
                .size(13),
            text(
                "Use the base URL. Full /chat/completions URLs are accepted and normalized.",
            )
            .size(11)
            .color(MUTED),
        ]
        .spacing(6);

        let model_input = column![
            text("Model").size(13).color(Color::from_rgb(0.72, 0.76, 0.84)),
            text_input(&preset.default_model, &self.form_model)
                .on_input(Message::ModelChanged)
                .padding(Padding::from(10))
                .size(13),
        ]
        .spacing(6);

        let api_key_input = column![
            text("API Key").size(13).color(Color::from_rgb(0.72, 0.76, 0.84)),
            text_input("sk-...", &self.form_api_key)
                .on_input(Message::ApiKeyChanged)
                .secure(true)
                .padding(Padding::from(10))
                .size(13),
            text(
                "Stored securely in the system vault, never written to config.toml.",
            )
            .size(11)
            .color(MUTED),
        ]
        .spacing(6);

        let test_result: Option<Element<Message>> = self.form_test_result.as_ref().map(|msg| {
            let is_err = msg.starts_with("Failed") || msg.starts_with("Error");
            container(text(msg).size(12).color(if is_err { ROSE } else { GREEN }))
                .padding(Padding::from(8))
                .style(move |_t: &Theme| container::Style {
                    background: Some(Background::Color(if is_err {
                        Color::from_rgba(0.89, 0.24, 0.35, 0.10)
                    } else {
                        Color::from_rgba(0.22, 0.78, 0.44, 0.10)
                    })),
                    border: Border {
                        radius: 8.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into()
        });

        let cancel_btn = button(text("Cancel").size(13).color(Color::from_rgb(0.72, 0.76, 0.84)))
            .padding(Padding::from(8))
            .style(|_t: &Theme, _status| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border {
                    color: Color::from_rgba(0.45, 0.48, 0.55, 0.60),
                    width: 1.0,
                    radius: 20.0.into(),
                },
                ..Default::default()
            })
            .on_press(Message::CloseModal);

        let test_btn = button(
            row![
                text(if self.form_testing { "\u{21BB}" } else { "\u{269B}" })
                    .size(13)
                    .color(Color::from_rgb(0.72, 0.76, 0.84)),
                text(if self.form_testing {
                    "Testing..."
                } else {
                    "Test Connection"
                })
                .size(13)
                .color(Color::from_rgb(0.72, 0.76, 0.84)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from(8))
        .style(|_t: &Theme, _status| button::Style {
            background: Some(Background::Color(Color::from_rgba(0.30, 0.32, 0.40, 0.60))),
            border: Border {
                color: Color::from_rgba(0.40, 0.43, 0.50, 0.50),
                width: 1.0,
                radius: 20.0.into(),
            },
            ..Default::default()
        })
        .on_press_maybe(if self.form_testing {
            None
        } else {
            Some(Message::TestConnection)
        });

        let save_btn = button(
            row![
                text("\u{1F4BE}").size(13).color(WHITE),
                text(if self.form_saving {
                    "Saving..."
                } else {
                    "Save Provider"
                })
                .size(13)
                .color(WHITE),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding(Padding::from(8))
        .style(|_t: &Theme, _status| button::Style {
            background: Some(Background::Color(ACCENT)),
            border: Border {
                radius: 20.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .on_press_maybe(if self.form_saving {
            None
        } else {
            Some(Message::SaveProvider)
        });

        let actions = row![
            cancel_btn,
            container("").width(Fill),
            test_btn,
            save_btn,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let mut form_body = column![
            row![
                column![
                    text(title).size(17).color(WHITE),
                    text(description).size(13).color(MUTED),
                ]
                .spacing(4)
                .width(Fill),
                button(text("\u{2715}").size(14).color(MUTED))
                    .style(|_t: &Theme, _status| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..Default::default()
                    })
                    .on_press(Message::CloseModal),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Start),
            endpoint_input,
            model_input,
            api_key_input,
        ]
        .spacing(16);

        if let Some(result_elem) = test_result {
            form_body = form_body.push(result_elem);
        }
        form_body = form_body.push(actions);

        let card = container(form_body)
            .padding(Padding::from(24))
            .max_width(520)
            .style(|_t: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.11, 0.18))),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 1.0,
                    radius: 16.0.into(),
                },
                shadow: Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.70),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 32.0,
                },
                ..Default::default()
            });

        container(card)
            .center_x(Fill)
            .center_y(Fill)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

// ---------------------------------------------------------------------------
// Toast element
// ---------------------------------------------------------------------------

impl ArgosApp {
    fn toast_element<'a>(&self, toast: &'a Toast) -> Element<'a, Message> {
        let accent = match toast.kind {
            ToastKind::Success => GREEN,
            ToastKind::Error => ROSE,
        };

        container(
            row![
                container("")
                    .width(Length::Fixed(4.0))
                    .height(Fill)
                    .style(move |_t: &Theme| container::Style {
                        background: Some(Background::Color(accent)),
                        border: Border {
                            radius: iced::border::Radius {
                                top_left: 10.0,
                                top_right: 0.0,
                                bottom_right: 0.0,
                                bottom_left: 10.0,
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                column![
                    text(&toast.title)
                        .size(13)
                        .color(WHITE)
                        .font(Font {
                            weight: iced::font::Weight::Semibold,
                            ..Font::default()
                        }),
                    text(&toast.message).size(12).color(MUTED),
                ]
                .spacing(2)
                .width(Fill),
                button(text("\u{2715}").size(12).color(MUTED))
                    .style(|_t: &Theme, _status| button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        ..Default::default()
                    })
                    .on_press(Message::DismissToast(toast.id)),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .padding(Padding {
                left: 0.0,
                right: 10.0,
                top: 10.0,
                bottom: 10.0,
            }),
        )
        .width(Length::Fixed(360.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(Color::from_rgba(0.08, 0.10, 0.16, 0.98))),
            border: Border {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.40),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        })
        .into()
    }
}

// ---------------------------------------------------------------------------
// Window icon
// ---------------------------------------------------------------------------

fn load_window_icon() -> Option<iced::window::Icon> {
    let svg_data = include_str!("../assets/icon.svg");
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg_data, &opt).ok()?;
    let size = 256u32;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(1.0, 1.0),
        &mut pixmap.as_mut(),
    );
    iced::window::icon::from_rgba(pixmap.take(), size, size).ok()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> iced::Result {
    let icon = load_window_icon();
    iced::application(ArgosApp::new, ArgosApp::update, ArgosApp::view)
        .theme(ArgosApp::theme)
        .window(iced::window::Settings {
            icon,
            ..Default::default()
        })
        .run()
}
