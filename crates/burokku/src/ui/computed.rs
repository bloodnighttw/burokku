#![allow(dead_code)]

use std::{collections::HashMap, ops::Deref};

use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout,
    geometry::{Point, Size},
    round_layout,
    style::{AvailableSpace, Display, Style},
    tree::{
        Cache, Layout, LayoutBlockContainer, LayoutFlexboxContainer, LayoutGridContainer,
        LayoutInput, LayoutOutput, LayoutPartialTree, NodeId as TaffyNodeId, RoundTree, RunMode,
        TraversePartialTree, TraverseTree,
    },
    BlockContext, CacheTree,
};

use crate::ui::elements::traits::Styles;

use super::elements::{styles::length::Dimension, DomSnapshot, Elements, NodeId};

/// Convert thread-safe authoritative DOM style data into Taffy data on MTS.
///
/// The returned value belongs to MTS computed state and is never published back
/// through the shared DOM snapshot.
pub fn taffy_style_for(element: &Elements) -> Style<String> {
    match element {
        Elements::Flex { style } => style.to_taffy_style(),
        Elements::Grid { style } => style.clone().to_taffy_style(),
        Elements::App => Style {
            display: Display::Block,
            size: Size {
                width: taffy::Dimension::percent(1.0),
                height: taffy::Dimension::percent(1.0),
            },
            ..Style::default()
        },
        Elements::Window { style } => {
            let mut result = Style {
                display: Display::Block,
                ..style.to_taffy_style()
            };
            if matches!(style.size.width, Dimension::Auto) {
                result.size.width = taffy::Dimension::percent(1.0);
            }
            if matches!(style.size.height, Dimension::Auto) {
                result.size.height = taffy::Dimension::percent(1.0);
            }
            result
        }
        Elements::Div { style } | Elements::Text { style } => {
            Style {
                display: Display::Block,
                ..style.to_taffy_style()
            }
        }
        Elements::_String { .. } => Style {
            display: Display::Block,
            ..Style::default()
        },
    }
}

/// One layout entry prepared for the later hit-testing phase.
///
/// `location` is absolute within the layout root rather than relative to the
/// parent, which prevents hit testing from having to walk the DOM tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitTestEntry {
    pub node: NodeId,
    pub order: u32,
    pub location: Point<f32>,
    pub size: Size<f32>,
}

/// Revision-tagged geometry used for scene construction and native hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct HitTestData {
    source_revision: u64,
    entries: Vec<HitTestEntry>,
}

impl HitTestData {
    pub fn source_revision(&self) -> u64 {
        self.source_revision
    }

    pub fn entries(&self) -> &[HitTestEntry] {
        &self.entries
    }

    /// Return the topmost node containing a logical viewport coordinate.
    ///
    /// Scene construction paints entries in traversal order, so testing in
    /// reverse order applies the same last-painted-wins policy. Right and
    /// bottom edges are exclusive to avoid two adjacent boxes claiming the
    /// same coordinate.
    pub fn hit_test(&self, point: Point<f32>) -> Option<NodeId> {
        self.entries.iter().rev().find_map(|entry| {
            let right = entry.location.x + entry.size.width;
            let bottom = entry.location.y + entry.size.height;
            (entry.size.width > 0.0
                && entry.size.height > 0.0
                && point.x >= entry.location.x
                && point.y >= entry.location.y
                && point.x < right
                && point.y < bottom)
                .then_some(entry.node)
        })
    }
}

/// One final Taffy layout tied to the committed DOM revision that produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevisionedLayout<'a> {
    source_revision: u64,
    node: NodeId,
    layout: &'a Layout,
}

impl RevisionedLayout<'_> {
    pub fn source_revision(&self) -> u64 {
        self.source_revision
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn layout(&self) -> &Layout {
        self.layout
    }
}

impl Deref for RevisionedLayout<'_> {
    type Target = Layout;

    fn deref(&self) -> &Self::Target {
        self.layout
    }
}

/// MTS-owned computed layout state.
///
/// A new low-level Taffy tree is built whenever the committed DOM revision
/// changes. Reusing a revision with different available space recalculates the
/// existing tree. The completed tree and hit-test geometry are replaced
/// together, so consumers cannot combine data from different DOM revisions.
#[derive(Debug, Default)]
pub struct ComputedState {
    current: Option<ComputedRevision>,
}

impl ComputedState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute layout from exactly the supplied committed snapshot.
    ///
    /// Returns `true` when layout was rebuilt or recalculated and `false` when
    /// both the source revision and available space were already current.
    pub fn compute_layout(
        &mut self,
        snapshot: &DomSnapshot,
        available_space: Size<AvailableSpace>,
    ) -> bool {
        let needs_rebuild = self
            .current
            .as_ref()
            .is_none_or(|current| current.source_revision != snapshot.revision());

        if needs_rebuild {
            let mut tree = LayoutTree::from_snapshot(snapshot);
            tree.compute_layout(available_space);
            let hit_test = tree.build_hit_test_data(snapshot.revision());
            self.current = Some(ComputedRevision {
                source_revision: snapshot.revision(),
                available_space,
                tree,
                hit_test,
            });
            return true;
        }

        let current = self
            .current
            .as_mut()
            .expect("a matching computed revision was checked above");
        if current.available_space == available_space {
            return false;
        }

        current.tree.compute_layout(available_space);
        current.available_space = available_space;
        current.hit_test = current.tree.build_hit_test_data(current.source_revision);
        true
    }

    /// DOM revision used by all currently exposed computed data.
    pub fn source_revision(&self) -> Option<u64> {
        self.current.as_ref().map(|current| current.source_revision)
    }

    pub fn available_space(&self) -> Option<Size<AvailableSpace>> {
        self.current.as_ref().map(|current| current.available_space)
    }

    /// Get a final, pixel-rounded layout by stable DOM handle.
    pub fn layout(&self, node: NodeId) -> Option<RevisionedLayout<'_>> {
        let current = self.current.as_ref()?;
        Some(RevisionedLayout {
            source_revision: current.source_revision,
            node,
            layout: current.tree.layout(node)?,
        })
    }

    /// Revision-tagged absolute geometry for future hit testing.
    pub fn hit_test_data(&self) -> Option<&HitTestData> {
        self.current.as_ref().map(|current| &current.hit_test)
    }

    pub fn node_count(&self) -> usize {
        self.current
            .as_ref()
            .map_or(0, |current| current.tree.nodes.len())
    }
}

#[derive(Debug)]
struct ComputedRevision {
    source_revision: u64,
    available_space: Size<AvailableSpace>,
    tree: LayoutTree,
    hit_test: HitTestData,
}

#[derive(Clone, Debug)]
struct ComputedNode {
    dom_id: NodeId,
    style: Style<String>,
    children: Vec<TaffyNodeId>,
    cache: Cache,
    unrounded_layout: Layout,
    final_layout: Layout,
}

/// A compact MTS-only tree implementing Taffy's low-level layout traits.
///
/// Taffy IDs are dense indexes into `nodes`. Stable DOM IDs are kept in the
/// reverse map and remain the only handles exposed outside computed state.
#[derive(Debug)]
struct LayoutTree {
    nodes: Vec<ComputedNode>,
    dom_to_taffy: HashMap<NodeId, TaffyNodeId>,
    root: TaffyNodeId,
}

impl LayoutTree {
    fn from_snapshot(snapshot: &DomSnapshot) -> Self {
        let dom = snapshot.dom();
        let mut nodes = Vec::new();
        let mut dom_to_taffy = HashMap::new();

        for (dom_id, element) in dom.iter() {
            let taffy_id = TaffyNodeId::from(nodes.len());
            dom_to_taffy.insert(dom_id, taffy_id);
            nodes.push(ComputedNode {
                dom_id,
                style: taffy_style_for(element),
                children: Vec::new(),
                cache: Cache::new(),
                unrounded_layout: Layout::new(),
                final_layout: Layout::new(),
            });
        }

        for node in &mut nodes {
            node.children = dom
                .children(node.dom_id)
                .expect("reachable DOM nodes have child lists")
                .iter()
                .map(|child| {
                    *dom_to_taffy
                        .get(child)
                        .expect("reachable DOM children were converted to Taffy nodes")
                })
                .collect();
        }

        let root = *dom_to_taffy
            .get(&dom.root())
            .expect("the DOM root is always reachable");
        Self {
            nodes,
            dom_to_taffy,
            root,
        }
    }

    fn node(&self, id: TaffyNodeId) -> &ComputedNode {
        &self.nodes[usize::from(id)]
    }

    fn node_mut(&mut self, id: TaffyNodeId) -> &mut ComputedNode {
        &mut self.nodes[usize::from(id)]
    }

    fn compute_layout(&mut self, available_space: Size<AvailableSpace>) {
        compute_root_layout(self, self.root, available_space);
        round_layout(self, self.root);
    }

    fn layout(&self, dom_id: NodeId) -> Option<&Layout> {
        let taffy_id = *self.dom_to_taffy.get(&dom_id)?;
        Some(&self.node(taffy_id).final_layout)
    }

    fn build_hit_test_data(&self, source_revision: u64) -> HitTestData {
        let mut entries = Vec::with_capacity(self.nodes.len());
        self.collect_hit_test_entries(self.root, Point::ZERO, &mut entries);
        HitTestData {
            source_revision,
            entries,
        }
    }

    fn collect_hit_test_entries(
        &self,
        node_id: TaffyNodeId,
        parent_location: Point<f32>,
        entries: &mut Vec<HitTestEntry>,
    ) {
        let node = self.node(node_id);
        let location = Point {
            x: parent_location.x + node.final_layout.location.x,
            y: parent_location.y + node.final_layout.location.y,
        };
        entries.push(HitTestEntry {
            node: node.dom_id,
            order: node.final_layout.order,
            location,
            size: node.final_layout.size,
        });

        for child in node.children.iter().copied() {
            self.collect_hit_test_entries(child, location, entries);
        }
    }

    fn compute_child_layout_with_context(
        &mut self,
        node_id: TaffyNodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node_id);
        }

        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let display = tree.node(node_id).style.display;
            let has_children = tree.child_count(node_id) > 0;

            match (display, has_children) {
                (Display::None, _) => compute_hidden_layout(tree, node_id),
                (Display::Block, true) => {
                    compute_block_layout(tree, node_id, inputs, block_context)
                }
                (Display::Flex, true) => compute_flexbox_layout(tree, node_id, inputs),
                (Display::Grid, true) => compute_grid_layout(tree, node_id, inputs),
                (_, false) => {
                    let style = &tree.node(node_id).style;
                    compute_leaf_layout(
                        inputs,
                        style,
                        |_value, _basis| 0.0,
                        |known_dimensions, _available_space| Size {
                            // Text shaping and other intrinsic measurement will
                            // replace these zero intrinsic dimensions later.
                            width: known_dimensions.width.unwrap_or(0.0),
                            height: known_dimensions.height.unwrap_or(0.0),
                        },
                    )
                }
            }
        })
    }
}

struct ChildIter<'a>(std::slice::Iter<'a, TaffyNodeId>);

impl Iterator for ChildIter<'_> {
    type Item = TaffyNodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

impl TraversePartialTree for LayoutTree {
    type ChildIter<'a>
        = ChildIter<'a>
    where
        Self: 'a;

    fn child_ids(&self, parent_node_id: TaffyNodeId) -> Self::ChildIter<'_> {
        ChildIter(self.node(parent_node_id).children.iter())
    }

    fn child_count(&self, parent_node_id: TaffyNodeId) -> usize {
        self.node(parent_node_id).children.len()
    }

    fn get_child_id(&self, parent_node_id: TaffyNodeId, child_index: usize) -> TaffyNodeId {
        self.node(parent_node_id).children[child_index]
    }
}

impl TraverseTree for LayoutTree {}

impl LayoutPartialTree for LayoutTree {
    type CoreContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type CustomIdent = String;

    fn get_core_container_style(&self, node_id: TaffyNodeId) -> Self::CoreContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn set_unrounded_layout(&mut self, node_id: TaffyNodeId, layout: &Layout) {
        self.node_mut(node_id).unrounded_layout = *layout;
    }

    fn compute_child_layout(&mut self, node_id: TaffyNodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_child_layout_with_context(node_id, inputs, None)
    }
}

impl CacheTree for LayoutTree {
    fn cache_get(&self, node_id: TaffyNodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        self.node(node_id).cache.get(inputs)
    }

    fn cache_store(&mut self, node_id: TaffyNodeId, inputs: &LayoutInput, output: LayoutOutput) {
        self.node_mut(node_id).cache.store(inputs, output);
    }

    fn cache_clear(&mut self, node_id: TaffyNodeId) {
        self.node_mut(node_id).cache.clear();
    }
}

impl LayoutBlockContainer for LayoutTree {
    type BlockContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_block_container_style(&self, node_id: TaffyNodeId) -> Self::BlockContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_block_child_style(&self, child_node_id: TaffyNodeId) -> Self::BlockItemStyle<'_> {
        &self.node(child_node_id).style
    }

    fn compute_block_child_layout(
        &mut self,
        node_id: TaffyNodeId,
        inputs: LayoutInput,
        block_context: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_child_layout_with_context(node_id, inputs, block_context)
    }
}

impl LayoutFlexboxContainer for LayoutTree {
    type FlexboxContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node_id: TaffyNodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_flexbox_child_style(&self, child_node_id: TaffyNodeId) -> Self::FlexboxItemStyle<'_> {
        &self.node(child_node_id).style
    }
}

impl LayoutGridContainer for LayoutTree {
    type GridContainerStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style<String>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: TaffyNodeId) -> Self::GridContainerStyle<'_> {
        &self.node(node_id).style
    }

    fn get_grid_child_style(&self, child_node_id: TaffyNodeId) -> Self::GridItemStyle<'_> {
        &self.node(child_node_id).style
    }
}

impl RoundTree for LayoutTree {
    fn get_unrounded_layout(&self, node_id: TaffyNodeId) -> Layout {
        self.node(node_id).unrounded_layout
    }

    fn set_final_layout(&mut self, node_id: TaffyNodeId, layout: &Layout) {
        self.node_mut(node_id).final_layout = *layout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::{
        styles::{
            common::CommonStyle,
            flex::FlexStyle,
            grid::{GridStyle, GridTemplateComponent, TrackSizingFunction},
            length::{Dimension, LengthPercentage},
        },
        BtsDom, SharedDom,
    };

    fn definite(width: f32, height: f32) -> Size<AvailableSpace> {
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        }
    }

    #[test]
    fn mts_converts_authoritative_styles_for_taffy() {
        let flex = Elements::Flex {
            style: Box::new(FlexStyle {
                common: CommonStyle {
                    flex_grow: 2.0,
                    ..CommonStyle::default()
                },
                ..FlexStyle::default()
            }),
        };
        let grid = Elements::Grid {
            style: Box::default(),
        };

        let flex = taffy_style_for(&flex);
        let grid = taffy_style_for(&grid);

        assert_eq!(flex.display, Display::Flex);
        assert_eq!(flex.flex_grow, 2.0);
        assert_eq!(grid.display, Display::Grid);
        assert_eq!(grid, GridStyle::default().to_taffy_style());
    }

    #[test]
    fn low_level_taffy_tree_calculates_grid_layout() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let (grid, first, second) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window {
                style: Box::default(),
            });
            let grid = dom.create(Elements::Grid {
                style: Box::new(GridStyle {
                    template_columns: vec![
                        GridTemplateComponent::Single(TrackSizingFunction::length(100.0)),
                        GridTemplateComponent::Single(TrackSizingFunction::fraction(1.0)),
                    ],
                    template_rows: vec![GridTemplateComponent::Single(
                        TrackSizingFunction::length(20.0),
                    )],
                    ..GridStyle::default()
                }),
            });
            let first = dom.create(Elements::Div {
                style: Box::default(),
            });
            let second = dom.create(Elements::Div {
                style: Box::default(),
            });
            dom.append_child(root, window).unwrap();
            dom.append_child(window, grid).unwrap();
            dom.append_child(grid, first).unwrap();
            dom.append_child(grid, second).unwrap();
            (grid, first, second)
        };
        owner.checkpoint().unwrap();
        let snapshot = shared.load();

        let mut computed = ComputedState::new();
        assert!(computed.compute_layout(&snapshot, definite(300.0, 200.0)));

        assert_eq!(computed.source_revision(), Some(snapshot.revision()));
        assert_eq!(computed.available_space(), Some(definite(300.0, 200.0)));
        let grid_layout = computed.layout(grid).unwrap();
        assert_eq!(grid_layout.source_revision(), snapshot.revision());
        assert_eq!(grid_layout.node(), grid);
        assert_eq!(grid_layout.layout().size.width, 300.0);
        assert_eq!(computed.layout(first).unwrap().size.width, 100.0);
        assert_eq!(computed.layout(second).unwrap().size.width, 200.0);
        assert_eq!(computed.layout(first).unwrap().size.height, 20.0);
        assert_eq!(computed.layout(second).unwrap().location.x, 100.0);
    }

    #[test]
    fn strongly_typed_styles_drive_visible_flex_layout() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let (first, second, third) = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window {
                style: Box::default(),
            });
            let row = dom.create(Elements::Flex {
                style: Box::new(FlexStyle {
                    gap: Size {
                        width: LengthPercentage::length(10.0),
                        height: LengthPercentage::length(10.0),
                    },
                    ..FlexStyle::default()
                }),
            });
            let child = |grow| Elements::Flex {
                style: Box::new(FlexStyle {
                    common: CommonStyle {
                        flex_basis: Dimension::length(0.0),
                        flex_grow: grow,
                        ..CommonStyle::default()
                    },
                    ..FlexStyle::default()
                }),
            };
            let first = dom.create(child(1.0));
            let second = dom.create(child(2.0));
            let third = dom.create(child(1.0));
            dom.append_child(root, window).unwrap();
            dom.append_child(window, row).unwrap();
            dom.append_child(row, first).unwrap();
            dom.append_child(row, second).unwrap();
            dom.append_child(row, third).unwrap();
            (first, second, third)
        };
        owner.checkpoint().unwrap();
        let snapshot = shared.load();
        let mut computed = ComputedState::new();

        computed.compute_layout(&snapshot, definite(400.0, 300.0));

        assert_eq!(computed.layout(first).unwrap().size.width, 95.0);
        assert_eq!(computed.layout(second).unwrap().size.width, 190.0);
        assert_eq!(computed.layout(third).unwrap().size.width, 95.0);
    }

    #[test]
    fn viewport_changes_recalculate_without_rebuilding_the_dom_revision() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let second = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window {
                style: Box::default(),
            });
            let grid = dom.create(Elements::Grid {
                style: Box::new(GridStyle {
                    template_columns: vec![
                        GridTemplateComponent::Single(TrackSizingFunction::length(100.0)),
                        GridTemplateComponent::Single(TrackSizingFunction::fraction(1.0)),
                    ],
                    ..GridStyle::default()
                }),
            });
            let first = dom.create(Elements::Div {
                style: Box::default(),
            });
            let second = dom.create(Elements::Div {
                style: Box::default(),
            });
            dom.append_child(root, window).unwrap();
            dom.append_child(window, grid).unwrap();
            dom.append_child(grid, first).unwrap();
            dom.append_child(grid, second).unwrap();
            second
        };
        owner.checkpoint().unwrap();
        let snapshot = shared.load();
        let mut computed = ComputedState::new();

        assert!(computed.compute_layout(&snapshot, definite(300.0, 200.0)));
        assert_eq!(computed.layout(second).unwrap().size.width, 200.0);
        assert!(!computed.compute_layout(&snapshot, definite(300.0, 200.0)));
        assert!(computed.compute_layout(&snapshot, definite(500.0, 200.0)));
        assert_eq!(computed.layout(second).unwrap().size.width, 400.0);
        assert_eq!(computed.source_revision(), Some(snapshot.revision()));
    }

    #[test]
    fn new_commits_replace_layout_and_hit_test_revision_together() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let grid = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window {
                style: Box::default(),
            });
            let grid = dom.create(Elements::Grid {
                style: Box::new(GridStyle {
                    template_columns: vec![GridTemplateComponent::Single(
                        TrackSizingFunction::length(100.0),
                    )],
                    ..GridStyle::default()
                }),
            });
            let child = dom.create(Elements::Div {
                style: Box::default(),
            });
            dom.append_child(root, window).unwrap();
            dom.append_child(window, grid).unwrap();
            dom.append_child(grid, child).unwrap();
            grid
        };
        owner.checkpoint().unwrap();
        let old = shared.load();

        let mut computed = ComputedState::new();
        computed.compute_layout(&old, definite(300.0, 200.0));
        assert_eq!(computed.source_revision(), Some(1));
        assert_eq!(computed.hit_test_data().unwrap().source_revision(), 1);

        owner
            .mutate()
            .set_element(
                grid,
                Elements::Grid {
                    style: Box::new(GridStyle {
                        template_columns: vec![GridTemplateComponent::Single(
                            TrackSizingFunction::length(50.0),
                        )],
                        ..GridStyle::default()
                    }),
                },
            )
            .unwrap();
        owner.checkpoint().unwrap();
        let new = shared.load();

        // Holding and recomputing the old Arc cannot pull in the newer commit.
        assert!(!computed.compute_layout(&old, definite(300.0, 200.0)));
        assert_eq!(computed.source_revision(), Some(old.revision()));
        assert_eq!(
            computed.hit_test_data().unwrap().source_revision(),
            old.revision()
        );

        assert!(computed.compute_layout(&new, definite(300.0, 200.0)));
        assert_eq!(computed.source_revision(), Some(new.revision()));
        assert_eq!(
            computed.hit_test_data().unwrap().source_revision(),
            new.revision()
        );
        assert_eq!(computed.node_count(), new.dom().iter().count());
    }

    #[test]
    fn hit_test_geometry_uses_absolute_locations_from_the_layout_revision() {
        let shared = SharedDom::new();
        let mut owner = BtsDom::new(shared.clone());
        let second = {
            let mut dom = owner.mutate();
            let root = dom.root();
            let window = dom.create(Elements::Window {
                style: Box::default(),
            });
            let grid = dom.create(Elements::Grid {
                style: Box::new(GridStyle {
                    template_columns: vec![
                        GridTemplateComponent::Single(TrackSizingFunction::length(75.0)),
                        GridTemplateComponent::Single(TrackSizingFunction::length(25.0)),
                    ],
                    template_rows: vec![GridTemplateComponent::Single(
                        TrackSizingFunction::length(10.0),
                    )],
                    ..GridStyle::default()
                }),
            });
            let first = dom.create(Elements::Div {
                style: Box::default(),
            });
            let second = dom.create(Elements::Div {
                style: Box::default(),
            });
            dom.append_child(root, window).unwrap();
            dom.append_child(window, grid).unwrap();
            dom.append_child(grid, first).unwrap();
            dom.append_child(grid, second).unwrap();
            second
        };
        owner.checkpoint().unwrap();
        let snapshot = shared.load();
        let mut computed = ComputedState::new();
        computed.compute_layout(&snapshot, definite(100.0, 100.0));

        let hit_test = computed.hit_test_data().unwrap();
        let entry = hit_test
            .entries()
            .iter()
            .find(|entry| entry.node == second)
            .unwrap();
        assert_eq!(hit_test.source_revision(), snapshot.revision());
        assert_eq!(entry.location.x, 75.0);
        assert_eq!(entry.size, computed.layout(second).unwrap().size);
    }

    #[test]
    fn hit_testing_uses_reverse_scene_order_and_half_open_edges() {
        let mut dom = super::super::elements::Dom::new();
        let back = dom.create(Elements::Div {
            style: Box::default(),
        });
        let front = dom.create(Elements::Div {
            style: Box::default(),
        });
        let hit_test = HitTestData {
            source_revision: 9,
            entries: vec![
                HitTestEntry {
                    node: back,
                    order: 0,
                    location: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                HitTestEntry {
                    node: front,
                    order: 1,
                    location: Point { x: 25.0, y: 25.0 },
                    size: Size {
                        width: 50.0,
                        height: 50.0,
                    },
                },
            ],
        };

        assert_eq!(hit_test.hit_test(Point { x: 50.0, y: 50.0 }), Some(front));
        assert_eq!(
            hit_test.hit_test(Point { x: 75.0, y: 50.0 }),
            Some(back),
            "the front box's right edge is exclusive"
        );
        assert_eq!(hit_test.hit_test(Point { x: 100.0, y: 50.0 }), None);
    }
}
