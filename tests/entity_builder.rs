extern crate alloc;
use alloc::string::String;
use flax::{component, Entity, Error, Exclusive, World};
use std::sync::Arc;

component! {
    a: i32,
    b: String,

    relation(id): Arc<()> => [ Exclusive ],
}

#[test]
fn entity_builder() {
    let mut world = World::new();

    let id1 = Entity::builder()
        .set(a(), 1)
        .set(b(), "hello".into())
        .spawn(&mut world);

    let mut id2 = Entity::builder();
    id2.set(a(), 2).set(b(), "hello".into());
    id2.remove(b());

    let id2 = id2.spawn(&mut world);

    assert_eq!(world.get(id2, a()).as_deref(), Ok(&2));
    assert_eq!(
        world.get(id2, b()).as_deref(),
        Err(&Error::MissingComponent(id2, b().info()))
    );

    let value = Arc::new(());

    let mut id3 = Entity::builder();
    id3.set(a(), 2);
    id3.set(b(), "world".into());
    id3.set(relation(id1), value.clone());

    assert_eq!(Arc::strong_count(&value), 2);

    id3.set(relation(id1), value.clone());

    assert_eq!(Arc::strong_count(&value), 2);

    let id3 = id3.spawn(&mut world);
    assert_eq!(Arc::strong_count(&value), 2);

    world.despawn(id3).unwrap();
    assert_eq!(Arc::strong_count(&value), 1);
}

#[test]
fn test_append() {
    let mut world = World::new();

    let id1 = Entity::builder()
        .set(a(), 1)
        .set(b(), "hello".into())
        .spawn(&mut world);

    let id2 = Entity::builder()
        .set(a(), 1)
        .set(b(), "hello".into())
        .spawn(&mut world);

    let id3 = world.spawn();

    let mut builder = Entity::builder();

    let value = Arc::new(());
    builder.set(a(), 5).set(relation(id2), value.clone());

    builder.append_to(&mut world, id3).unwrap();

    assert_eq!(Arc::strong_count(&value), 2);
    assert!(world.has(id3, relation(id2)));
    assert!(!world.has(id3, relation(id1)));

    let mut builder = Entity::builder();

    builder.set(relation(id1), value.clone());

    assert_eq!(Arc::strong_count(&value), 3);

    builder.append_to(&mut world, id3).unwrap();
    // The old relation is dropped
    assert_eq!(Arc::strong_count(&value), 2);
    assert!(!world.has(id3, relation(id2)));
    assert!(world.has(id3, relation(id1)));
}
