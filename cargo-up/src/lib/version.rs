use crate::{
    normalize,
    ra_hir::Semantics,
    ra_ide_db::RootDatabase,
    ra_syntax::ast,
    semver::{SemVerError, Version as SemverVersion},
    Upgrader,
};
use std::collections::BTreeMap as Map;

macro_rules! alias {
    ($node:ident) => {
        type $node = dyn Fn(&mut Upgrader, &ast::$node, &Semantics<RootDatabase>);
    };
}

macro_rules! hook {
    ($method:ident, $node:ident) => {
        pub fn $method<F>(mut self, f: F) -> Self
        where
            F: Fn(&mut Upgrader, &ast::$node, &Semantics<RootDatabase>) + 'static,
        {
            self.$method.push(Box::new(f));
            self
        }
    };
}

alias!(MethodCallExpr);
alias!(FieldExpr);

pub struct Version {
    pub(crate) version: SemverVersion,
    pub(crate) peers: Vec<String>,
    pub(crate) rename_methods: Map<String, Map<String, String>>,
    pub(crate) rename_members: Map<String, Map<String, String>>,
    pub(crate) hook_method_call_expr: Vec<Box<MethodCallExpr>>,
    pub(crate) hook_field_expr: Vec<Box<FieldExpr>>,
}

impl Version {
    pub fn new(version: &str) -> Result<Self, SemVerError> {
        Ok(Self {
            version: SemverVersion::parse(version)?,
            peers: vec![],
            rename_methods: Map::new(),
            rename_members: Map::new(),
            hook_method_call_expr: vec![],
            hook_field_expr: vec![],
        })
    }

    pub fn peers(mut self, peers: &[&str]) -> Self {
        self.peers = peers.to_vec().iter().map(|x| normalize(*x)).collect();
        self
    }

    pub fn rename_methods(mut self, name: &str, map: &[[&str; 2]]) -> Self {
        self.rename_methods.insert(
            name.to_string(),
            map.iter()
                .map(|x| (x[0].to_string(), x[1].to_string()))
                .collect(),
        );
        self
    }

    pub fn rename_members(mut self, name: &str, map: &[[&str; 2]]) -> Self {
        self.rename_members.insert(
            name.to_string(),
            map.iter()
                .map(|x| (x[0].to_string(), x[1].to_string()))
                .collect(),
        );
        self
    }

    hook!(hook_method_call_expr, MethodCallExpr);
    hook!(hook_field_expr, FieldExpr);
}
