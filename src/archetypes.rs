use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

use crate::{
    archetype::Archetype,
    component::{dummy, ComponentKey},
    entity::{EntityKind, EntityStore, EntityStoreIter, EntityStoreIterMut},
    events::EventSubscriber,
    metadata::exclusive,
    ArchetypeId, ComponentDesc, Entity,
};

// fn is_sorted<T: Ord>(v: &[T]) -> bool {
//     v.windows(2).all(|w| w[0] < w[1])
// }

pub(crate) struct Archetypes {
    pub(crate) root: ArchetypeId,
    pub(crate) reserved: ArchetypeId,
    gen: u32,
    inner: EntityStore<Archetype>,

    // These trickle down to the archetypes
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    pub(crate) index: ArchetypeIndex,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());
        let reserved = archetypes.spawn(Archetype::empty());

        Self {
            root,
            inner: archetypes,
            gen: 2,
            reserved,
            subscribers: Vec::new(),
            index: ArchetypeIndex::new(),
        }
    }

    pub fn get(&self, arch_id: ArchetypeId) -> &Archetype {
        match self.inner.get(arch_id) {
            Some(v) => v,
            None => {
                panic!("Invalid archetype: {arch_id}");
            }
        }
    }

    pub fn get_mut(&mut self, arch_id: ArchetypeId) -> &mut Archetype {
        let arch = self.inner.get_mut(arch_id).expect("Invalid archetype");

        arch
    }

    /// Prunes a leaf and its ancestors from empty archetypes
    pub(crate) fn prune_arch(&mut self, arch_id: ArchetypeId) -> bool {
        let arch = self.get(arch_id);
        if arch_id == self.root
            || arch_id == self.reserved
            || !arch.is_empty()
            || !arch.outgoing.is_empty()
        {
            return false;
        }

        let arch = self.inner.despawn(arch_id).unwrap();
        self.index.unregister(arch_id, &arch);

        for (&key, &dst_id) in &arch.incoming {
            let dst = self.get_mut(dst_id);
            dst.remove_link(key);

            self.prune_arch(dst_id);
        }

        self.gen = self.gen.wrapping_add(1);

        true
    }

    /// Returns or creates an archetype which satisfies all the given components
    ///
    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    ///
    /// Ensures the `exclusive` property of any relations are satisfied
    pub(crate) fn find_create(
        &mut self,
        components: impl IntoIterator<Item = ComponentDesc>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.root;

        for head in components {
            let cur = &mut self.inner.get(cursor).expect("Invalid archetype id");

            cursor = match cur.outgoing.get(&head.key) {
                Some(&id) => id,
                None => {
                    // Create archetypes as we go and build the tree
                    let arch_components = cur.components_desc().chain([head]);

                    // Ensure exclusive property of the new component are maintained
                    let mut new = if head.is_relation() && head.meta_ref().has(exclusive()) {
                        // Remove any existing components of the same relation
                        // `head` is always a more recently added component since an
                        // archetype with it does not exist (yet)
                        Archetype::new(
                            arch_components
                                .filter(|v| v.key.id != head.key.id || v.key == head.key),
                        )
                    } else {
                        Archetype::new(arch_components)
                    };

                    // Insert the appropriate subscribers
                    for s in &self.subscribers {
                        if s.matches_arch(&new) {
                            new.add_handler(s.clone())
                        }
                    }

                    // Increase gen
                    self.gen = self.gen.wrapping_add(1);
                    let new_id = self.inner.spawn(new);

                    let (cur, new) = self.inner.get_disjoint(cursor, new_id).unwrap();
                    cur.add_child(head.key, new_id);
                    new.add_incoming(head.key, cursor);

                    self.index.register(new_id, new);

                    new_id
                }
            };
        }

        (cursor, self.inner.get_mut(cursor).unwrap())
    }

    pub fn root(&self) -> ArchetypeId {
        self.root
    }

    pub fn get_disjoint(
        &mut self,
        a: Entity,
        b: Entity,
    ) -> Option<(&mut Archetype, &mut Archetype)> {
        let (a, b) = self.inner.get_disjoint(a, b)?;

        Some((a, b))
    }

    pub fn iter(&self) -> EntityStoreIter<Archetype> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> EntityStoreIterMut<Archetype> {
        self.inner.iter_mut()
    }

    /// Despawn an archetype, leaving a hole in the tree.
    ///
    /// It is the callers responibility to cleanup child nodes if the node is internal
    /// Children are detached from the tree, but still accessible by id
    pub fn despawn(&mut self, id: ArchetypeId) -> Archetype {
        let arch = self.inner.despawn(id).expect("Despawn invalid archetype");

        // Remove outgoing edges
        for (&component, &dst_id) in &arch.incoming {
            let dst = self.get_mut(dst_id);
            dst.remove_link(component);
        }
        self.gen = self.gen.wrapping_add(1);

        self.index.unregister(id, &arch);
        arch
    }

    pub fn add_subscriber(&mut self, subscriber: Arc<dyn EventSubscriber>) {
        // Prune subscribers
        self.subscribers.retain(|v| v.is_connected());

        for (_, arch) in self.inner.iter_mut() {
            if subscriber.matches_arch(arch) {
                arch.add_handler(subscriber.clone());
            }
        }

        self.subscribers.push(subscriber)
    }

    pub(crate) fn gen(&self) -> u32 {
        self.gen
    }
}

struct ArchetypeRecord {
    // arch_id: ArchetypeId,
    cell_index: usize,
    /// The number of relations for this component.
    ///
    /// Since they are ordered sequentially, they start at `cell_index` and continue for `relation_count`
    relation_count: usize,
}

type ArchetypeRecords = BTreeMap<ArchetypeId, ArchetypeRecord>;
pub struct ArchetypeIndex {
    components: BTreeMap<ComponentKey, ArchetypeRecords>,
}

impl ArchetypeIndex {
    pub fn new() -> Self {
        Self {
            components: BTreeMap::new(),
        }
    }

    fn register_relation(&mut self, arch_id: ArchetypeId, key: ComponentKey, cell_index: usize) {
        let records = self
            .components
            .entry(key)
            .or_default()
            .entry(arch_id)
            .or_insert(ArchetypeRecord {
                cell_index,
                relation_count: 0,
            });

        records.relation_count += 1;
    }

    fn unregister_relation(&mut self, arch_id: ArchetypeId, key: ComponentKey) {
        let records = self.components.get_mut(&key).unwrap();
        let record = records.get_mut(&arch_id).unwrap();

        record.relation_count -= 1;
        if record.relation_count == 0 {
            records.remove(&arch_id);
        }
    }

    pub fn register(&mut self, arch_id: ArchetypeId, arch: &Archetype) {
        profile_function!();
        for (&key, &cell_index) in arch.components() {
            if key.is_relation() {
                assert!(key.object.is_some());
                self.register_relation(arch_id, ComponentKey::new(dummy(), key.object), cell_index);
                self.register_relation(
                    arch_id,
                    ComponentKey::new(key.id(), Some(dummy())),
                    cell_index,
                );
            }

            self.components.entry(key).or_default().insert(
                arch_id,
                ArchetypeRecord {
                    cell_index,
                    relation_count: 0,
                },
            );
        }
    }

    pub fn unregister(&mut self, arch_id: ArchetypeId, arch: &Archetype) {
        profile_function!();
        for (key, _) in arch.components() {
            if key.is_relation() {
                assert!(key.object.is_some());
                self.unregister_relation(arch_id, ComponentKey::new(dummy(), key.object));
                self.unregister_relation(arch_id, ComponentKey::new(key.id(), Some(dummy())));
            }

            let records = self.components.get_mut(key).unwrap();
            records.remove(&arch_id);
            if records.is_empty() {
                self.components.remove(&key);
            }
        }
    }
}
