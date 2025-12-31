Converts a series of expressions into a `StrBuf`. Unlike `print()`, `str()` does
not terminate the string with a newline.

`str()` will create a *new* `StrBuf`. For example, consider:

```poniescript
class Horse {
    var name: StrBuf = "Twilight";
}

var horse1 = new Horse {};
// Now horse2.name is pointing at the same StrBuf as horse1.name.
var horse2 = new Horse { name: horse1.name };

// This, however, does not modify horse2.name, because it reassigns horse1.name
// to an entirely new StrBuf.
horse1.name = str("Starlight");
```

