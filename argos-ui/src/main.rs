mod backend;
mod services;
mod workspace;

fn main() -> iced::Result {
    iced::application(
        workspace::WorkspaceApp::new,
        workspace::WorkspaceApp::update,
        workspace::WorkspaceApp::view,
    )
    .theme(workspace::WorkspaceApp::theme)
    .run()
}
