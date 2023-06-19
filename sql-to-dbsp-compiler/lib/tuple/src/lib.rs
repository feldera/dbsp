//! Rust tuples only go up to 12 fields, but we may need more.

#[macro_export]
macro_rules! declare_tuples {
    (
        $(
            $tuple_name:ident<$($element:tt),* $(,)?>
        ),*
        $(,)?
    ) => {
        $(
            #[derive(Default, Eq, Ord, Clone, Hash, PartialEq, PartialOrd, Serialize, Deserialize, SizeOf, Decode, Encode)]
            pub struct $tuple_name<$($element,)*>($(pub $element,)*);

            impl<$($element),*> $tuple_name<$($element,)*>
            {
                fn new($($element: $element),*) -> Self {
                    Self($($element),*)
                }
            }

	    #[cfg(test)]
            impl<$($element),*> ToSqlRow for $tuple_name<$($element,)*>
            where
                $(SqlValue: From<$element>,)*
                $($element: Clone,)*
            {
                fn to_row(&self) -> SqlRow
                {
                    let mut result = SqlRow::new();
                    let $tuple_name($($element),*) = self;
                    $(result.push(SqlValue::from($element.clone()));)*
                    result
                }
            }

            impl<$($element),*> From<($($element,)*)> for $tuple_name<$($element,)*> {
                fn from(($($element,)*): ($($element,)*)) -> Self {
                    Self($($element),*)
                }
            }

            impl<$($element),*> Into<($($element,)*)> for $tuple_name<$($element,)*> {
                fn into(self) -> ($($element,)*) {
                    let $tuple_name($($element),*) = self;
                    ($($element,)*)
                }
            }

            impl<$($element: Debug),*> Debug for $tuple_name<$($element),*> {
                fn fmt(&self, f: &mut Formatter) -> FmtResult {
                    let $tuple_name($($element),*) = self;
                    f.debug_tuple("")
                        $(.field(&$element))*
                        .finish()
                }
            }

            impl<$($element: Copy),*> Copy for $tuple_name<$($element),*> {}
        )*
    };
}
