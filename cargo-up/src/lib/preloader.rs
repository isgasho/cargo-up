use crate::{
    ra_hir::{Adt, AssocItem, Crate, Field, Function, Module, ModuleDef, ScopeDef},
    ra_ide_db::RootDatabase,
    INTERNAL_ERR,
};
use std::collections::HashMap as Map;

#[derive(Debug, Default)]
pub(crate) struct Preloader {
    pub(crate) methods: Map<Function, String>,
    pub(crate) members: Map<Field, String>,
    pub(crate) visited: Vec<String>,
}

impl Preloader {
    pub(crate) fn load(&mut self, name: &str, db: &RootDatabase, krate: &Crate) {
        if self.visited.iter().any(|x| x == name) {
            return;
        }

        println!("Pre loading {}", name);

        let module = krate.root_module(db).expect(INTERNAL_ERR);
        self.load_module(db, &module, vec![name.to_string()]);

        self.visited.push(name.to_string());
    }

    fn load_module(&mut self, db: &RootDatabase, module: &Module, path: Vec<String>) {
        // Load struct members
        for item in module.scope(db, None) {
            if let ScopeDef::ModuleDef(ModuleDef::Adt(Adt::Struct(s))) = item.1 {
                let name = format!("{}::{}", path.join("::"), s.name(db));

                for field in s.fields(db) {
                    self.members.insert(field, name.clone());
                }
            }
        }

        for impl_def in module.impl_defs(db) {
            let target_ty = impl_def.target_ty(db);

            match target_ty.as_adt() {
                // Load struct methods
                Some(Adt::Struct(s)) => {
                    let name = format!("{}::{}", path.join("::"), s.name(db));

                    for assoc_item in impl_def.items(db) {
                        if let AssocItem::Function(f) = assoc_item {
                            self.methods.insert(f, name.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        for child in module.children(db) {
            if let Some(name) = child.name(db) {
                let mut path = path.clone();
                path.push(format!("{}", name));

                self.load_module(db, &child, path);
            }
        }
    }
}