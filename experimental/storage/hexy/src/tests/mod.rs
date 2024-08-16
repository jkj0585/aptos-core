// Copyright (c) Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{
    in_mem::{base::HexyBase, overlay::HexyOverlay},
    utils::sort_dedup,
    LeafIdx, ARITY,
};
use aptos_crypto::{
    hash::{CryptoHasher, HexyHasher, HOT_STATE_PLACE_HOLDER_HASH},
    HashValue,
};
use aptos_infallible::Mutex;
use itertools::Itertools;
use proptest::{collection::vec, prelude::*};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn arb_test_case() -> impl Strategy<Value = (LeafIdx, Vec<Vec<(LeafIdx, HashValue)>>)> {
    (1u32..1000, 1usize..100).prop_flat_map(|(num_leaves, num_batches)| {
        (
            Just(num_leaves),
            vec(
                vec((0..(num_leaves as LeafIdx), any::<HashValue>()), 0..100),
                num_batches,
            ),
        )
    })
}

fn naive_root_hash(num_leaves: LeafIdx, updates: &[Vec<(LeafIdx, HashValue)>]) -> HashValue {
    let mut hashes = vec![*HOT_STATE_PLACE_HOLDER_HASH; num_leaves as usize];
    let all_updates = updates
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeMap<_, _>>();
    all_updates
        .into_iter()
        .for_each(|(idx, hash)| hashes[idx as usize] = hash);

    while hashes.len() > 1 {
        hashes = hashes
            .into_iter()
            .chunks(ARITY)
            .into_iter()
            .map(|chunk| {
                let mut children = chunk.into_iter().collect_vec();
                children.resize_with(ARITY, || *HOT_STATE_PLACE_HOLDER_HASH);

                if children
                    .iter()
                    .all(|hash| hash == &*HOT_STATE_PLACE_HOLDER_HASH)
                {
                    *HOT_STATE_PLACE_HOLDER_HASH
                } else {
                    let mut hasher = HexyHasher::default();
                    for child in children {
                        hasher.update(child.as_slice())
                    }
                    hasher.finish()
                }
            })
            .collect_vec()
    }

    hashes
        .first()
        .cloned()
        .unwrap_or(*HOT_STATE_PLACE_HOLDER_HASH)
}

fn sleep_random() {
    let micros = rand::random::<u64>() % 100;
    std::thread::sleep(std::time::Duration::from_micros(micros));
}

fn update_fn(
    base: Arc<HexyBase>,
    first_pending: Arc<Mutex<HexyOverlay>>,
    latest: Arc<Mutex<HexyOverlay>>,
    updates: Vec<Vec<(LeafIdx, HashValue)>>,
) -> impl FnOnce() {
    move || {
        for batch in updates.into_iter() {
            let view = latest.lock().view(&base, &first_pending.lock());
            sleep_random();
            *latest.lock() = view.new_overlay(batch.clone()).unwrap();
        }
    }
}

fn merge_fn(
    base: Arc<HexyBase>,
    first_pending: Arc<Mutex<HexyOverlay>>,
    latest: Arc<Mutex<HexyOverlay>>,
    quit_signal: Arc<AtomicBool>,
) -> impl FnOnce() {
    move || {
        let mut quit = false;
        while !quit {
            quit = quit_signal.load(Ordering::Acquire);
            sleep_random();

            let bottom = first_pending.lock().clone();
            let top = latest.lock().clone();
            if top.is_the_same(&bottom) {
                continue;
            }
            base.merge(top.overlay.into_layers_view_since(bottom.overlay.clone()))
                .unwrap();
            *first_pending.lock() = bottom;
        }
    }
}

proptest! {
    #[test]
    fn test_sort_dedup(data in vec(any::<(u16, u16)>(), 0..100)) {
        let sort_debuped = sort_dedup(data.clone());
        let expected = data.into_iter().collect::<BTreeMap<_, _>>().into_iter().collect_vec();

        assert_eq!(sort_debuped, expected);
    }

    #[test]
    fn test_update((num_leaves, updates) in arb_test_case()) {
        let base = Arc::new(HexyBase::allocate(num_leaves));
        let root_overlay = HexyOverlay::new_empty(&base);
        let first_pending = Arc::new(Mutex::new(root_overlay.clone()));
        let latest = Arc::new(Mutex::new(root_overlay));
        let quit_signal = Arc::new(AtomicBool::new(false));

        let root_hash = naive_root_hash(num_leaves, &updates);

        let update_thread = std::thread::spawn(
            update_fn(base.clone(), first_pending.clone(), latest.clone(), updates)
        );
        let merge_thread = std::thread::spawn(
            merge_fn(base.clone(), first_pending.clone(), latest.clone(), quit_signal.clone())
        );

        update_thread.join().unwrap();
        prop_assert_eq!(latest.lock().root_hash, root_hash);

        quit_signal.store(true, Ordering::Release);
        merge_thread.join().unwrap();
        prop_assert_eq!(base.root_hash(), root_hash);
    }
}