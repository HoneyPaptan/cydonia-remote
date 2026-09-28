use artifact::article::properties;
use gui::model::sink::Write;
use remote::proto::{Action, Cover, File};

pub fn action(project: String, write: Write) -> Option<Action> {
    Some(match write {
        Write::CreateBoard { name, key } => Action::CreateBoard { project, name, key },
        Write::SaveBoard(board) => Action::SaveBoard {
            project,
            board: toml::to_string(&board).ok()?,
        },
        Write::RemoveBoard(id) => Action::RemoveBoard { project, id },
        Write::CreateArticle(markdown) => Action::CreateArticle { project, markdown },
        Write::WriteArticle { id, markdown } => Action::WriteArticle {
            project,
            id,
            markdown,
        },
        Write::SaveProperties {
            id,
            properties: held,
        } => Action::SaveProperties {
            project,
            id,
            properties: properties::apply("", &held).unwrap_or_default(),
        },
        Write::RemoveArticle(id) => Action::RemoveArticle { project, id },
        Write::SetCover { id, cover } => Action::SetCover {
            project,
            id,
            cover: cover.map(|(name, bytes)| Cover {
                name,
                file: File(bytes),
            }),
        },
    })
}
