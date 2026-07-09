use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredPackage;
use crate::extensions::fixtures::{
    DiscoveredFixture, FixtureScope, HasFixtures, NormalizedFixture, RequiresFixtures,
    get_auto_use_fixtures,
};

/// Shared cache of normalized fixture *definitions* for a whole test run.
///
/// Keyed by fully-qualified fixture name. This stores the static dependency
/// graph (`NormalizedFixture`), not runtime values — those live in
/// [`super::FixtureCache`] and remain scoped so function-scoped values stay
/// fresh per test.
pub(super) type NormalizedFixtureCache = RefCell<HashMap<String, Rc<NormalizedFixture>>>;

/// Resolves fixtures at runtime during test execution.
///
/// Unlike pre-normalization, this resolver finds and normalizes fixtures
/// on-demand when tests need them. `current` is typed as a trait object so
/// callers may pass either a test module (normal test / module-autouse
/// resolution), a conftest module (package-autouse resolution), or the
/// session package itself (session-autouse resolution) — the latter gives
/// session-level autouse fixtures visibility into `framework_module` via
/// the `HasFixtures` impl on `DiscoveredPackage`.
///
/// Normalized definitions are stored in a run-wide [`NormalizedFixtureCache`]
/// so modules share conftest/framework graphs, and a per-resolver short-name
/// index avoids repeating parent walks within the same lookup context.
pub(super) struct RuntimeFixtureResolver<'a> {
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
    /// Run-wide cache of fully built fixture graphs, keyed by qualified name.
    normalized_cache: &'a NormalizedFixtureCache,
    /// Short-name → normalized fixture for this `(parents, current)` context.
    /// Within one resolution context fixture short names are unique (first
    /// match wins), so this lets repeated lookups skip `find_fixture`.
    local_by_name: HashMap<String, Rc<NormalizedFixture>>,
}

impl<'a> RuntimeFixtureResolver<'a> {
    pub(super) fn new(
        parents: &'a [&'a DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
        normalized_cache: &'a NormalizedFixtureCache,
    ) -> Self {
        Self {
            parents,
            current,
            normalized_cache,
            local_by_name: HashMap::new(),
        }
    }

    /// Normalize a fixture and its dependencies recursively.
    ///
    /// Always caches the resulting graph, including function-scoped fixtures.
    /// Freshness of runtime values (e.g. a new `tmp_path` per test) is handled
    /// by [`super::FixtureCache`]'s function-scope clearing — not by rebuilding
    /// the static dependency graph.
    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &DiscoveredFixture,
    ) -> Rc<NormalizedFixture> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.normalized_cache.borrow().get(&cache_key) {
            return Rc::clone(cached);
        }

        let required_fixtures: Vec<String> = fixture.required_fixtures(py);
        let dependent_fixtures = self.get_dependent_fixtures(py, Some(fixture), &required_fixtures);

        let result = Rc::new(NormalizedFixture {
            name: fixture.name().clone(),
            dependencies: dependent_fixtures,
            scope: fixture.scope(),
            is_generator: fixture.is_generator(),
            py_function: Rc::new(fixture.function().clone_ref(py)),
            stmt_function_def: Rc::clone(fixture.stmt_function_def()),
            source_file: fixture.source_file().clone(),
        });

        self.normalized_cache
            .borrow_mut()
            .insert(cache_key, Rc::clone(&result));

        result
    }

    /// Get normalized auto-use fixtures for a given scope.
    pub(super) fn get_normalized_auto_use_fixtures(
        &mut self,
        py: Python,
        scope: FixtureScope,
    ) -> Vec<Rc<NormalizedFixture>> {
        let auto_use_fixtures = get_auto_use_fixtures(self.parents, self.current, scope);

        auto_use_fixtures
            .into_iter()
            .map(|fixture| {
                let short_name = fixture.name().function_name().to_string();
                let normalized = self.normalize_fixture(py, fixture);
                self.local_by_name
                    .insert(short_name, Rc::clone(&normalized));
                normalized
            })
            .collect()
    }

    /// Resolve fixture dependencies for a test, excluding parametrize params.
    pub(super) fn resolve_test_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
        parametrize_param_names: &HashSet<&str>,
    ) -> Vec<Rc<NormalizedFixture>> {
        let regular_fixture_names: Vec<String> = fixture_names
            .iter()
            .filter(|name| !parametrize_param_names.contains(name.as_str()))
            .cloned()
            .collect();

        self.get_dependent_fixtures(py, None, &regular_fixture_names)
    }

    /// Resolve `use_fixtures` dependencies.
    pub(super) fn resolve_use_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
    ) -> Vec<Rc<NormalizedFixture>> {
        self.get_dependent_fixtures(py, None, fixture_names)
    }

    /// Get dependent fixtures for a list of fixture names.
    fn get_dependent_fixtures(
        &mut self,
        py: Python,
        current_fixture: Option<&DiscoveredFixture>,
        fixture_names: &[String],
    ) -> Vec<Rc<NormalizedFixture>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());

        for dep_name in fixture_names {
            // Self-referential parameters (a fixture that lists itself) must
            // still go through `find_fixture`, which skips the current fixture
            // and may resolve a parent of the same short name.
            let is_self_ref = current_fixture
                .is_some_and(|fixture| fixture.name().function_name() == dep_name.as_str());

            if !is_self_ref && let Some(cached) = self.local_by_name.get(dep_name) {
                normalized_fixtures.push(Rc::clone(cached));
                continue;
            }

            if let Some(fixture) =
                find_fixture(current_fixture, dep_name, self.parents, self.current)
            {
                let normalized = self.normalize_fixture(py, fixture);
                // Only record the short-name mapping when it is not a shadowed
                // self-reference; the local index is for the context's first
                // match of that name.
                if !is_self_ref {
                    self.local_by_name
                        .insert(dep_name.clone(), Rc::clone(&normalized));
                }
                normalized_fixtures.push(normalized);
            }
        }

        normalized_fixtures
    }
}

/// Finds a fixture by name, searching in the current node and parent packages.
/// We pass in the current fixture to avoid returning it (which would cause infinite recursion).
fn find_fixture<'a>(
    current_fixture: Option<&DiscoveredFixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
) -> Option<&'a DiscoveredFixture> {
    if let Some(fixture) = current.get_fixture(name)
        && current_fixture.is_none_or(|current_fixture| current_fixture.name() != fixture.name())
    {
        return Some(fixture);
    }

    for parent in parents {
        if let Some(fixture) = parent.get_fixture(name)
            && current_fixture
                .is_none_or(|current_fixture| current_fixture.name() != fixture.name())
        {
            return Some(fixture);
        }
    }

    None
}
