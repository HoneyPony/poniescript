# `DynArray[T]`

`DynArray` is the fundamental dynamic (i.e. resizeable) array type in PonieScript.
If you are using an array, it is quite likely you will be using a `DynArray`.

The goal of the PonieScript `DynArray` is to be easy to use. However, there is
a lot of subtlety with how `DynArray` works in a multi-threaded environment, so
be sure to read the relevant parts of the documentation when working in such
an environment.

## Member index

- `var length: int`
- `fun push(object: T) -> void`
- `operator[]: fun(index: int) -> T`
- `operator[]=: fun(index: int, value: T) -> T`

## Member reference

### `var length: int`

The length of the `DynArray`. This is distinct from the number of elements allocated
for its backing storage. However, for correctness reasons, and memory safety
reasons, any index that is >= this length value is not allowed to be accessed
(i.e. accesses will panic).

The length cannot be written to, only read from.

### `fun push(object: T) -> void`

Pushes an object to the end of the `DynArray`, and increases its length by
one.

This method is amortized O(1). In most cases, it will simply push the element
to the existing backing storage, and increase the length by one. However, if
there is no more space left in the existing backing storage, then the backing
storage must be re-allocated and the old elements copied.

For memory safety reasons, this reallocation cannot (for now) simply re-use the 
existing backing storage. So, the copy will always occur.

Note that the way this method interacts with iterations over the `DynArray`
is very subtle. It is in general recommended to avoid iterating over the `DynArray`
and pushing to it at the same time, especially in a concurrent context.

However, it is safe to do so in a single-threaded context. That is, code
along the lines of:

```poniescript
for elem in dynarray {
    if rand() < 0.3 { dynarray.push(5); }
    print(elem);
}
```

Will safely iterate over every element of the array exactly once.

## Creating a `DynArray`

The only way currently available to create a `DynArray` is through an array
literal, with a properly typed variable. That is:

```poniescript
var x = [1, 2, 3]; // Wrong; will create an Array[int].
var y: DynArray[int] = [1, 2, 3]; // Correct!
var z = []; // Wrong; type cannot be inferred as Array or DynArray.
var w: DynArray[int] = []; // Correct!
```

In the future, we may have additional array literal prefixes/suffixes that enable
creating a `DynArray` without spelling out its type; or maybe more sophisticated
type inference to do the same.

In any case, you currently must spell out the entire type at least once to use
a `DynArray`. Of course, type inference still works for other variables:

```poniescript
var y: DynArray[int] = [1, 2, 3];
var other = y; // Correct!
```

## Usage

Much like `Array`, `DynArray` can be indexed and has a `.length` property, enabling
C-style usage:

```poniescript
fun print_all(arr: DynArray[int]) {
    for i in 0..arr.length {
        print(arr[i]);
    }
}
```

What makes `DynArray` *dynamic* is that additional elements can be added over time,
i.e. its length can change.

```
var arr: DynArray[int] = [1, 2, 3];
print(arr.length); // 3
arr.push(5);
print(arr.length); // 4
```

