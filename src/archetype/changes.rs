use std::fmt::Display;

use itertools::Itertools;

use crate::ComponentInfo;

use super::{Slice, Slot};

#[derive(Clone, PartialEq, Eq)]
/// A self compacting change tracking which holds either singular changes or a
/// range of changes, automatically merging adjacent ones.
///
///
/// The changes are always stored in a non-overlapping ascending order.
pub struct Changes {
    info: ComponentInfo,
    inner: Vec<Change>,
}

impl std::fmt::Debug for Changes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Changes")
            .field("name", &self.info.name())
            .field("inner", &self.inner)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
/// Represents a change for a slice of entities for a specific component
pub enum ChangeKind {
    /// Component was modified
    Modified,
    /// Component was inserted
    Inserted,
    /// Component was removed
    Removed,
}

impl Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeKind::Modified => f.write_str("modified"),
            ChangeKind::Inserted => f.write_str("inserted"),
            ChangeKind::Removed => f.write_str("removed"),
        }
    }
}

impl ChangeKind {
    /// Returns `true` if the change kind is [`Remove`].
    ///
    /// [`Remove`]: ChangeKind::Removed
    #[must_use]
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Returns `true` if the change kind is [`Insert`].
    ///
    /// [`Insert`]: ChangeKind::Inserted
    #[must_use]
    pub fn is_inserted(&self) -> bool {
        matches!(self, Self::Inserted)
    }

    /// Returns `true` if the change kind is [`ChangeKind::Modified`]
    ///
    /// [`Modified`]: ChangeKind::Modified
    #[must_use]
    pub fn is_modified(&self) -> bool {
        matches!(self, Self::Modified)
    }

    #[cfg(test)]
    pub(crate) fn is_modified_or_inserted(&self) -> bool {
        self.is_modified() || self.is_inserted()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
/// Represents a change over a slice of entities in an archetype which ocurred
/// at a specific time.
pub struct Change {
    /// The slice of entities in the archetype which are affected
    pub slice: Slice,
    /// The world tick of the change event
    pub tick: u32,
    /// The kind of change
    pub kind: ChangeKind,
}

impl Change {
    /// Creates a new change
    pub(crate) fn new(slice: Slice, tick: u32, kind: ChangeKind) -> Self {
        Self { slice, tick, kind }
    }

    /// Create a new modification event
    pub(crate) fn modified(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Modified,
        }
    }

    /// Create a new insert event
    pub(crate) fn inserted(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Inserted,
        }
    }

    /// Create a new remove event
    pub(crate) fn removed(slice: Slice, tick: u32) -> Change {
        Self {
            slice,
            tick,
            kind: ChangeKind::Removed,
        }
    }
}

fn is_sorted<T>(a: &[T]) -> bool
where
    T: Ord,
{
    !a.windows(2).any(|v| v[0] > v[1])
}
impl Changes {
    pub(crate) fn new(info: ComponentInfo) -> Self {
        Self {
            info,
            inner: Default::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn as_set(&self, f: impl Fn(&Change) -> bool) -> std::collections::BTreeSet<Slot> {
        self.iter()
            .filter_map(|v| if f(v) { Some(v.slice) } else { None })
            .flatten()
            .collect()
    }

    // #[cfg(test)]
    // pub(crate) fn as_map(&self) -> std::collections::BTreeMap<Slot, (u32, ChangeKind)> {
    //     self.inner
    //         .iter()
    //         .flat_map(|v| v.slice.iter().map(move |p| (p, (v.tick, v.kind))))
    //         .collect()
    // }
    #[cfg(debug_assertions)]
    pub(crate) fn assert_ordered(&self, msg: &str) {
        let modified = self
            .inner
            .iter()
            .filter(|v| v.kind == ChangeKind::Modified)
            .map(|v| v.slice)
            .collect_vec();
        let inserted = self
            .inner
            .iter()
            .filter(|v| v.kind == ChangeKind::Inserted)
            .map(|v| v.slice)
            .collect_vec();
        let removed = self
            .inner
            .iter()
            .filter(|v| v.kind == ChangeKind::Removed)
            .map(|v| v.slice)
            .collect_vec();

        if !is_sorted(&modified) {
            panic!(
                "Modified not sorted: {modified:?}. Found: {:#?}\n\n{msg}",
                self.inner
            );
        }
        if !is_sorted(&inserted) {
            panic!(
                "Inserted not sorted: {inserted:?}. Found: {:#?}\n\n{msg}",
                self.inner
            );
        }
        if !is_sorted(&removed) {
            panic!(
                "Removed not sorted: {removed:?}. Found: {:#?}\n\n{msg}",
                self.inner
            );
        }
    }

    pub(crate) fn set(&mut self, change: Change) -> &mut Self {
        let mut insert_point = 0;
        let mut i = 0;
        let mut joined = false;

        #[cfg(debug_assertions)]
        self.assert_ordered("Not sorted at beginning");

        self.inner.retain_mut(|v| {
            // Remove older changes which are a subset of the newer slots
            if v.kind == change.kind && v.tick < change.tick {
                if let Some(diff) = v.slice.difference(change.slice) {
                    v.slice = diff;
                }
            }

            // Merge the change into an already existing change
            // Do not change start as that will invalidate ordering
            if v.slice.start < change.slice.start && v.tick == change.tick && v.kind == change.kind
            {
                // Merge atop change of the same change
                if let Some(u) = v.slice.union(&change.slice) {
                    joined = true;
                    v.slice = u;
                }
            }

            if v.slice.is_empty() {
                return false;
            }

            i += 1;
            if v.kind == change.kind && v.slice < change.slice {
                insert_point = i;
            }

            true
        });

        if !joined {
            self.inner.insert(insert_point, change);
        }

        #[cfg(debug_assertions)]
        self.assert_ordered(&format!("Not sorted after `set` inserting: {change:?}"));

        self
    }

    pub(crate) fn migrate_to(&mut self, other: &mut Self, src_slot: Slot, dst_slot: Slot) {
        for mut removed in self.remove(src_slot) {
            // Change the slot
            removed.slice = Slice::single(dst_slot);
            other.set(removed);
        }
    }

    /// Removes `src` by swapping `dst` into its place
    pub(crate) fn swap_out(&mut self, src: Slot, dst: Slot) -> Vec<Change> {
        let src_changes = self.remove(src);
        let dst_changes = self.remove(dst);

        for mut v in dst_changes.into_iter() {
            assert_eq!(v.slice, Slice::single(dst));
            v.slice = Slice::single(src);
            self.set(v);
        }

        src_changes
    }

    /// Removes a slot from the change list
    pub fn remove(&mut self, slot: Slot) -> Vec<Change> {
        let slice = Slice::single(slot);
        let mut result = Vec::with_capacity(self.inner.capacity());

        let mut right: Vec<Change> = Vec::new();

        // =====-=====
        //    ==-=========
        //     =-===
        //
        // =====
        //    ==
        //     =
        //
        // right: ====, =========, ===

        // ====
        //   ==
        //    =
        //      ====
        //      =========
        //      ===

        #[cfg(debug_assertions)]
        self.assert_ordered("Not sorted before `remove`");

        let old = self.inner.clone();

        let removed = self
            .inner
            .drain(..)
            .flat_map(|v| {
                if let Some((l, _, r)) = v.slice.split_with(&slice) {
                    if !l.is_empty() {
                        // If the pending elements are smaller, push them first
                        if let Some(r) = right.first() {
                            if r.slice < l {
                                result.append(&mut right);
                            }
                        }

                        result.push(Change::new(l, v.tick, v.kind));
                    }
                    if !r.is_empty() {
                        right.push(Change::new(r, v.tick, v.kind));
                    }

                    Some(Change::new(slice, v.tick, v.kind))
                } else {
                    // If the pending elements are smaller, push them first
                    if let Some(r) = right.first() {
                        if r.slice < v.slice {
                            result.append(&mut right);
                        }
                    }

                    result.push(v);
                    None
                }
            })
            .collect_vec();

        result.append(&mut right);

        self.inner = result;
        #[cfg(debug_assertions)]
        self.assert_ordered(&format!(
            "Not sorted after `remove` while removing: {slot}\n\n{old:#?}"
        ));
        removed
    }

    /// Returns the changes in the change list at a particular index.
    pub fn get(&self, index: usize) -> Option<&Change> {
        self.inner.get(index)
    }

    /// Returns the number of changes
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[must_use]
    /// Returns true if the change list is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate all changes in ascending order
    pub fn iter(&self) -> std::slice::Iter<Change> {
        self.inner.iter()
    }

    #[cfg(test)]
    pub(crate) fn as_changed_set(&self, tick: u32) -> std::collections::BTreeSet<Slot> {
        self.as_set(|v| v.kind.is_modified_or_inserted() && v.tick > tick)
    }

    pub(crate) fn info(&self) -> ComponentInfo {
        self.info
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::*;

    crate::component! {
        a: (),
    }

    #[test]
    fn changes() {
        let mut changes = Changes::new(a().info());

        changes.set(Change::modified(Slice::new(0, 5), 1));

        changes.set(Change::modified(Slice::new(70, 92), 2));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 5), 1),
                Change::modified(Slice::new(70, 92), 2)
            ]
        );

        changes.set(Change::modified(Slice::new(3, 5), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 3), 1),
                Change::modified(Slice::new(3, 5), 3),
                Change::modified(Slice::new(70, 92), 2),
            ]
        );

        // Extend previous change
        changes.set(Change::modified(Slice::new(4, 14), 3));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 3), 1),
                Change::modified(Slice::new(3, 14), 3),
                Change::modified(Slice::new(70, 92), 2),
            ]
        );

        // Overwrite almost all
        changes.set(Change::modified(Slice::new(0, 89), 4));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [
                Change::modified(Slice::new(0, 89), 4),
                Change::modified(Slice::new(89, 92), 2),
            ]
        );
    }

    #[test]
    fn changes_small() {
        let mut changes = Changes::new(a().info());

        for i in 0..239 {
            let perm = (i * (i + 2)) % 300;
            // let perm = i;
            changes.set(Change::modified(Slice::single(perm), i as _));
        }

        changes.set(Change::modified(Slice::new(70, 249), 300));
        changes.set(Change::modified(Slice::new(0, 89), 301));
        changes.set(Change::modified(Slice::new(209, 300), 302));

        eprintln!("Changes: {changes:#?}");
    }

    #[test]
    fn adjacent() {
        let mut changes = Changes::new(a().info());

        changes.set(Change::modified(Slice::new(0, 63), 1));
        changes.set(Change::modified(Slice::new(63, 182), 1));

        assert_eq!(
            changes.iter().copied().collect_vec(),
            [Change::modified(Slice::new(0, 182), 1)]
        );
    }

    #[test]
    fn migrate() {
        let mut changes_1 = Changes::new(a().info());
        let mut changes_2 = Changes::new(a().info());

        changes_1
            .set(Change::modified(Slice::new(20, 48), 1))
            .set(Change::modified(Slice::new(32, 98), 2));

        assert_eq!(
            changes_1.inner,
            [
                Change::modified(Slice::new(20, 32), 1),
                Change::modified(Slice::new(32, 98), 2)
            ]
        );

        changes_1.migrate_to(&mut changes_2, 25, 67);

        assert_eq!(
            changes_1.inner,
            [
                Change::modified(Slice::new(20, 25), 1),
                Change::modified(Slice::new(26, 32), 1),
                Change::modified(Slice::new(32, 98), 2)
            ]
        );

        assert_eq!(changes_2.inner, [Change::modified(Slice::single(67), 1)])
    }
}
