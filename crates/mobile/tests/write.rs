use artifact::{article::properties::Properties, board::Board};
use cydonia_mobile::write;
use gui::model::sink::Write;
use remote::proto::Action;

fn board() -> Board {
    toml::from_str("id = \"road\"\nname = \"Roadmap\"\nkey = \"ROAD\"\n").unwrap()
}

#[test]
fn a_saved_board_travels_as_toml_the_laptop_can_read_back() {
    let Some(Action::SaveBoard {
        project,
        board: text,
    }) = write::action("/work/p".into(), Write::SaveBoard(board()))
    else {
        panic!("expected a board save");
    };
    assert_eq!(project, "/work/p");
    let read: Board = toml::from_str(&text).unwrap();
    assert_eq!((read.id.as_str(), read.name.as_str()), ("road", "Roadmap"));
}

#[test]
fn every_write_names_its_project() {
    let writes = vec![
        Write::CreateBoard {
            name: "A".into(),
            key: "A".into(),
        },
        Write::RemoveBoard("road".into()),
        Write::CreateArticle("# A".into()),
        Write::WriteArticle {
            id: "1".into(),
            markdown: "# B".into(),
        },
        Write::SaveProperties {
            id: "1".into(),
            properties: Properties::default(),
        },
        Write::RemoveArticle("1".into()),
    ];
    for written in writes {
        let action = write::action("/work/p".into(), written).unwrap();
        assert_eq!(action.written_project(), Some("/work/p"));
    }
}
