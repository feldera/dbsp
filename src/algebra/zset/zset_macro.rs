/// Allows easily creating
/// [`OrdIndexedZSet`](crate::trace::ord::OrdIndexedZSet)s
#[macro_export]
macro_rules! indexed_zset {
    ( $($key:expr => { $($value:expr => $weight:expr),* }),* $(,)?) => {{
        let mut batcher = <<$crate::trace::ord::OrdIndexedZSet<_, _, _> as $crate::trace::Batch>::Batcher as $crate::trace::Batcher<_, _, _, _, _>>::new(());
        let mut batch = vec![ $( $( (($key, $value), $weight) ),* ),* ];
        $crate::trace::Batcher::push_batch(&mut batcher, &mut batch);
        $crate::trace::Batcher::seal(batcher)
    }};
}

/// Allows easily creating [`OrdZSet`](crate::trace::ord::OrdZSet)s
#[macro_export]
macro_rules! zset {
    ( $( $key:expr => $weight:expr ),* $(,)?) => {{
        let mut batcher = <<$crate::trace::ord::OrdZSet<_, _> as $crate::trace::Batch>::Batcher as $crate::trace::Batcher<_, _, _, _, _>>::new(());

        let mut batch = vec![ $( (($key, ()), $weight) ),* ];
        $crate::trace::Batcher::push_batch(&mut batcher, &mut batch);
        $crate::trace::Batcher::seal(batcher)
    }};
}
