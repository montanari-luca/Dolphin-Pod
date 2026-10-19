//! The user's cosmetic roles: default "Master", validation, ordering.

use std::sync::Arc;

use dolphin_core::Core;
use dolphin_core::db::Db;
use dolphin_core::model::{AgentColor, UserRole};
use dolphin_core::secrets::MemoryStore;

fn role(name: &str, color: AgentColor) -> UserRole {
    UserRole { id: String::new(), name: name.into(), icon: "star".into(), color, sort: 0, updated_at: 0 }
}

#[test]
fn default_master_role_and_custom_roles() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let roles = core.list_roles().unwrap();
    assert_eq!(roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Master"]);

    let builder = core.save_role(role("Builder", AgentColor::solid("#22c55e"))).unwrap();
    assert_eq!(builder.sort, 1, "new roles go to the end");
    assert!(core.save_role(role("master", AgentColor::solid("#ffffff"))).is_err(), "names are unique");
    assert!(core.save_role(role("  ", AgentColor::solid("#ffffff"))).is_err());
    assert!(core.save_role(role("Bad", AgentColor::solid("green"))).is_err());

    // Putting Builder first makes it the role that colors the user's name.
    let ordered = core.reorder_roles(vec![builder.id.clone(), roles[0].id.clone()]).unwrap();
    assert_eq!(ordered[0].name, "Builder");

    core.delete_role(&builder.id).unwrap();
    assert_eq!(core.list_roles().unwrap().len(), 1);
}
