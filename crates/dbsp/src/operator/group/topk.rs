use super::{DiffGroupTransformer, Monotonicity, NonIncrementalGroupTransformer};
use crate::{
    algebra::ZRingValue, trace::Cursor, DBData, DBWeight, IndexedZSet, OrdIndexedZSet, RootCircuit,
    Stream,
};
use std::marker::PhantomData;

impl<B> Stream<RootCircuit, B>
where
    B: IndexedZSet + Send,
{
    /// Pick `k` smallest values in each group.
    ///
    /// For each key in the input stream, removes all but `k` smallest values.
    #[allow(clippy::type_complexity)]
    pub fn topk_asc(&self, k: usize) -> Stream<RootCircuit, OrdIndexedZSet<B::Key, B::Val, B::R>>
    where
        B::R: ZRingValue,
    {
        self.group_transform(DiffGroupTransformer::new(TopK::asc(k)))
    }

    /// Pick `k` largest values in each group.
    ///
    /// For each key in the input stream, removes all but `k` largest values.
    #[allow(clippy::type_complexity)]
    pub fn topk_desc(&self, k: usize) -> Stream<RootCircuit, OrdIndexedZSet<B::Key, B::Val, B::R>>
    where
        B::R: ZRingValue,
    {
        self.group_transform(DiffGroupTransformer::new(TopK::desc(k)))
    }
}

struct TopK<I, R, const ASCENDING: bool> {
    k: usize,
    name: String,
    // asc: bool,
    _phantom: PhantomData<(I, R)>,
}

impl<I, R> TopK<I, R, true> {
    fn asc(k: usize) -> Self {
        Self {
            k,
            name: format!("top-{k}-asc"),
            _phantom: PhantomData,
        }
    }
}

impl<I, R> TopK<I, R, false> {
    fn desc(k: usize) -> Self {
        Self {
            k,
            name: format!("top-{k}-desc"),
            _phantom: PhantomData,
        }
    }
}

impl<I, R, const ASCENDING: bool> NonIncrementalGroupTransformer<I, I, R> for TopK<I, R, ASCENDING>
where
    I: DBData,
    R: DBWeight,
{
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn monotonicity(&self) -> Monotonicity {
        if ASCENDING {
            Monotonicity::Ascending
        } else {
            Monotonicity::Descending
        }
    }

    fn transform<C, CB>(&mut self, cursor: &mut C, mut output_cb: CB)
    where
        C: Cursor<I, (), (), R>,
        CB: FnMut(I, R),
    {
        let mut count = 0usize;

        if ASCENDING {
            while cursor.key_valid() && count < self.k {
                let w = cursor.weight();
                if !w.is_zero() {
                    output_cb(cursor.key().clone(), w);
                    count += 1;
                }
                cursor.step_key();
            }
        } else {
            cursor.fast_forward_keys();

            while cursor.key_valid() && count < self.k {
                let w = cursor.weight();
                if !w.is_zero() {
                    output_cb(cursor.key().clone(), w);
                    count += 1;
                }
                cursor.step_key_reverse();
            }
        }
    }
}
