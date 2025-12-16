Prints out any series of expressions. Each expression is printed to the console
in order. The print will be terminated by a newline.

`print()` does not add any whitespace besides the terminating newline. If you wish
to separate arguments, you should intersperse them manually. For example:
```poniescript
class Horse { name: StrBuf = "Twilight"; age: int = 30; }
var horse = new Horse{};
print(horse.name, " ", horse.age); // prints "Twilight 30"
```

If you wish to convert a series of expressions into a [`StrBuf`], use [`str()`] instead.

### Returns
print() always returns the result of its first argument. This allows you to
intersperse print() with existing logic, such as:
```poniescript
if print(a > b) {
    do_high_a_logic();
}
```