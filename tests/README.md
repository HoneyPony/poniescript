# PonieScript integration tests

The integration tests test the compiler end-to-end, with one of the following goals:

1. Observing a correct write to standard output
2. Observing a specific error message

This is accomplished through two special kinds of comments in the source code.

To test outputs, use `//!`. Each of these comments denotes exactly one line expected in the output. Additionally, the expected text is always trimmed. This means you can test lines of output like so:

```poniescript
fun init() {
    print("hello"); //! hello
    print("world"); //! world
}
```

Notice that we do not have to explicitly list the newline anywhere. Also notice that we do nonetheless *expect* there to be a newline.

To test error messages, use `//?`. Each of these denotes exactly one expected error message. So, for example,
```poniescript
//? Expected ')' after parenthesized expression, got ';'
//? Expected 'var', 'const', 'class', or 'fun', got '<EOF>'
var x = 2 * (3 + 4;
```

Note that these are exact error messages. So in this example, although we're really trying to test the ')' error message, we must also include a second error message for the missing keyword. If the parser is ever updated to not report that second error, we will want to remove it from this test.

This does mean that the error tests are much more fragile than the exepcted output tests. In particular, the expected output tests should keep working barring a change in actual language semantics, while the error tests will break just due to a change in error reporting (and this can be in the parser, the typechecker, etc). So, the intended path of development is that, whenever an error message is changed, simply update the relevant tests at that time.

## Future work

Some other test conditions we could consider including in the future include:

1. Checking that certain symbols do / do not appear in the output (e.g. to test constant expression folding and similar)
2. Checking that certain variables are inferred to be specific types
3. Checking any other details of the code structure
