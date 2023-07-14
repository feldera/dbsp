# Floating point types

We support standard IEEE 754 floating point types.

`double` is a 64-bit standard FP value.  Accepted synonyms are
`float8` and `float64`.

`float` is a 32-bit standard FP value.  Accepted synonyms are
`float4`, and `float32`.

Floating point values include special values, such as `NaN` (not a
number), `-Infinity`, and `-Infinity`.  An alternative spelling for
`-Infinity` is `-inf`, and an alternative spelling for `Infinity` is
`inf`, and an alternative spelling for 'NaN' is 'nan'.  When written
as SQL literals, these values have to be surrounded by simple quotes:
`'inf'`.  Please note that these strings are case-sensitive and spaces
are ignored.

Infinity plus any finite value equals Infinity, as does Infinity plus
Infinity.  Infinity minus `Infinity` yields `NaN`.

`NaN` (not a number) value is used to represent undefined results.
An operation with a `NaN` input yields `NaN`.  The only exception
is when the operation's output does not depend on the `NaN` value:
an example is `NaN` raised to the zero power yields one.

In sorting order `NaN` is considered greater than all other values.

The legal operations are `+` (plus, unary and binary), `-` (minus,
unary and binary), `*` (multiplication), `/` (division).
(modulus).

Division or modulus by zero return `NaN`.

Casting a string to a floating-point value will produce the value
`0` when parsing fails.

Casting a value that is out of the supported range to a floating
point type will produce a value that is `inf` or `-inf`.

Please note that numeric values with a decimal point have the
`decimal` type by default.  To write a floating-point literal you have
to include the `e` for exponent using the following grammar:

digits`.`digits[`e`[`+-`]digits]

[digits]`.`digits[`e`[`+-`]digits]

Alternatively, you can use an explicit cast:

```SQL
REAL '1.23'  -- string style
1.23::REAL   -- PostgreSQL style
```

## Predefined functions on Floating-point Values

<table>
  <tr>
    <td><code>ABS(value)</code></td>
    <td>absolute value</td>
  </tr>
  <tr>
    <td><code>CEIL(value)</code></td>
    <td>Ceiling function: nearest integer value greater than or equal to argument (result is a floating point value)</td>
  </tr>
  <tr>
    <td><code>FLOOR(value)</code></td>
    <td>Floor function: nearest integer value less than or equal to argument (result is a floating point value)</td>
  </tr>
  <tr>
    <td><code>POWER(BASE, EXPONENT)</code></td>
    <td>The power function, raising <code>BASE</code> to the power <code>EXPONENT</code></td>
  </tr>
  <tr>
    <td><code>SQRT(value)</code></td>
    <td>Square root of value. Produces a runtime error for negative values.</td>
  </tr>
  <tr>
    <td><code>LN(value)</code></td>
    <td>The natural logarithm of value. Produces a runtime error for values less than or equal to zero.</td>
  </tr>
  <tr>
    <td><code>LOG10(value)</code></td>
    <td>The logarithm base 10 of value. Produces a runtime error for values less than or equal to zero.</td>
  </tr>
</table>
