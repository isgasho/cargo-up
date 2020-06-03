use crate::{
    ra_hir::{AssocItem, Crate, PathResolution, Semantics},
    ra_ide_db::{symbol_index::SymbolsDatabase, RootDatabase},
    ra_syntax::{ast, AstNode},
    semver::{SemVerError, Version as SemverVersion},
    utils::{normalize, Error, INTERNAL_ERR, TERM_ERR, TERM_OUT},
    Preloader, Upgrader, Version, Visitor,
};
use ra_db::{FileId, SourceDatabaseExt};
use ra_text_edit::TextEdit;
use rust_analyzer::cli::load_cargo;
use std::{
    collections::BTreeMap as Map,
    fs::{read_to_string, write},
    io::Result as IoResult,
    path::Path,
};

pub struct Runner {
    pub(crate) minimum: Option<SemverVersion>,
    pub(crate) versions: Vec<Version>,
    upgrader: Upgrader,
    version: SemverVersion,
    preloader: Preloader,
    current_file: Option<FileId>,
}

impl Runner {
    pub fn new() -> Self {
        Self {
            minimum: None,
            versions: vec![],
            upgrader: Upgrader::default(),
            version: SemverVersion::parse("0.0.0").expect(INTERNAL_ERR),
            preloader: Preloader::default(),
            current_file: None,
        }
    }

    pub fn minimum(mut self, version: &str) -> Result<Self, SemVerError> {
        self.minimum = Some(SemverVersion::parse(version)?);
        Ok(self)
    }

    pub fn version(mut self, version: Version) -> Self {
        self.versions.push(version);
        self
    }
}

impl Runner {
    #[doc(hidden)]
    pub fn run(
        &mut self,
        root: &Path,
        dep: &str,
        from: SemverVersion,
        to: SemverVersion,
    ) -> IoResult<()> {
        let (host, source_roots) = load_cargo(root, true, false).unwrap();
        let db = host.raw_database();

        let mut changes = Map::<FileId, TextEdit>::new();
        let semantics = Semantics::new(db);

        if let Some(min) = self.minimum.clone() {
            if from < min {
                return Error::NotMinimum(dep.into(), min.to_string()).print_err();
            }
        }

        self.version = to;
        let version = self.get_version();

        let peers = if let Some(version) = version {
            let mut peers = vec![dep.to_string()];
            peers.extend(version.peers.clone());
            peers
        } else {
            return Error::NoChanges(self.version.to_string()).print_out();
        };

        // Loop to find and eager load the dep we are upgrading
        for krate in Crate::all(db) {
            let file_id = krate.root_file(db);
            let source_root = db.source_root(db.file_source_root(file_id));

            if !source_root.is_library {
                continue;
            }

            if let Some(name) = krate.display_name(db) {
                if let Some(peer) = peers
                    .iter()
                    .find(|x| **x == normalize(&format!("{}", name)))
                {
                    self.preloader.load(peer, db, &krate);
                }
            }
        }

        // Actual loop to walk through the source code
        for source_root_id in db.local_roots().iter() {
            let source_root = db.source_root(*source_root_id);

            for file_id in source_root.walk() {
                self.current_file = Some(file_id);
                let source_file = semantics.parse(file_id);
                // println!("{:#?}", source_file.syntax());

                self.visit(source_file.syntax(), &semantics);

                changes.insert(file_id, self.upgrader.finish());
            }
        }

        // Apply changes
        for (file_id, edit) in changes {
            let full_path = db.file_relative_path(file_id).to_path(
                source_roots
                    .get(&db.file_source_root(file_id))
                    .expect(INTERNAL_ERR)
                    .path(),
            );

            let mut file_text = read_to_string(&full_path)?;

            edit.apply(&mut file_text);
            write(&full_path, file_text)?;
        }

        TERM_ERR.flush()?;
        TERM_OUT.flush()?;

        Ok(())
    }

    fn get_version(&self) -> Option<&Version> {
        self.versions.iter().find(|x| x.version == self.version)
    }
}

impl Visitor for Runner {
    fn visit_source_file(&mut self, _: &ast::SourceFile, _: &Semantics<RootDatabase>) {}

    fn visit_method_call_expr(
        &mut self,
        method_call_expr: &ast::MethodCallExpr,
        semantics: &Semantics<RootDatabase>,
    ) {
        let mut upgrader = self.upgrader.clone();
        let version = self.get_version().expect(INTERNAL_ERR);

        for hook in &version.hook_method_call_expr {
            hook(&mut upgrader, method_call_expr, semantics);
        }

        if let Some(name_ref) = method_call_expr.name_ref() {
            let method = name_ref.text().to_string();

            // Filter out methods which don't have the same names we are looking for
            if !version
                .rename_methods
                .iter()
                .any(|x| x.1.iter().any(|y| *y.0 == method))
            {
                return;
            }

            if let Some(f) = semantics.resolve_method_call(method_call_expr) {
                if let Some(name) = self
                    .preloader
                    .methods
                    .iter()
                    .find(|x| *x.0 == f)
                    .map(|x| x.1)
                {
                    if let Some(map) = version.rename_methods.get(name) {
                        if let Some(to) = map.get(&method) {
                            upgrader.replace(name_ref.syntax().text_range(), to.to_string());
                        }
                    }
                }
            }
        }

        self.upgrader = upgrader;
    }

    fn visit_call_expr(&mut self, call_expr: &ast::CallExpr, semantics: &Semantics<RootDatabase>) {
        let mut upgrader = self.upgrader.clone();
        let version = self.get_version().expect(INTERNAL_ERR);

        for hook in &version.hook_call_expr {
            hook(&mut upgrader, call_expr, semantics);
        }

        self.upgrader = upgrader;
    }

    fn visit_path_expr(&mut self, path_expr: &ast::PathExpr, semantics: &Semantics<RootDatabase>) {
        let mut upgrader = self.upgrader.clone();
        let version = self.get_version().expect(INTERNAL_ERR);

        for hook in &version.hook_path_expr {
            hook(&mut upgrader, path_expr, semantics);
        }

        if let Some(path) = path_expr.path() {
            if let Some(segment) = path.segment() {
                if let Some(name_ref) = segment.name_ref() {
                    let method = name_ref.text().to_string();

                    // Filter out methods which don't have the same names we are looking for
                    if !version
                        .rename_methods
                        .iter()
                        .any(|x| x.1.iter().any(|y| *y.0 == method))
                    {
                        return;
                    }

                    if let Some(PathResolution::AssocItem(AssocItem::Function(f))) =
                        semantics.resolve_path(&path)
                    {
                        if let Some(name) = self
                            .preloader
                            .methods
                            .iter()
                            .find(|x| *x.0 == f)
                            .map(|x| x.1)
                        {
                            if let Some(map) = version.rename_methods.get(name) {
                                if let Some(to) = map.get(&method) {
                                    upgrader
                                        .replace(name_ref.syntax().text_range(), to.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        self.upgrader = upgrader;
    }

    fn visit_field_expr(
        &mut self,
        field_expr: &ast::FieldExpr,
        semantics: &Semantics<RootDatabase>,
    ) {
        let mut upgrader = self.upgrader.clone();
        let version = self.get_version().expect(INTERNAL_ERR);

        for hook in &version.hook_field_expr {
            hook(&mut upgrader, field_expr, semantics);
        }

        if let Some(name_ref) = field_expr.name_ref() {
            let method = name_ref.text().to_string();

            // Filter out members which don't have the same names we are looking for
            if !version
                .rename_members
                .iter()
                .any(|x| x.1.iter().any(|y| *y.0 == method))
            {
                return;
            }

            if let Some(f) = semantics.resolve_field(field_expr) {
                if let Some(name) = self
                    .preloader
                    .members
                    .iter()
                    .find(|x| *x.0 == f)
                    .map(|x| x.1)
                {
                    if let Some(map) = version.rename_members.get(name) {
                        if let Some(to) = map.get(&method) {
                            upgrader.replace(name_ref.syntax().text_range(), to.to_string());
                        }
                    }
                }
            }
        }

        self.upgrader = upgrader;
    }

    fn visit_record_field(
        &mut self,
        record_field: &ast::RecordField,
        semantics: &Semantics<RootDatabase>,
    ) {
        let mut upgrader = self.upgrader.clone();
        let version = self.get_version().expect(INTERNAL_ERR);

        for hook in &version.hook_record_field {
            hook(&mut upgrader, record_field, semantics);
        }

        self.upgrader = upgrader;
    }
}
