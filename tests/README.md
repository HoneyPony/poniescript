# PonieScript integration tests

The integration tests test the compiler end-to-end, with one of the following goals:

1. Observing a correct write to standard output
2. Observing a specific error code

For now, neither of these cases are possible. But, here is the plan for generating tests in the future:

1. Create the associated .poni file
2. Create a new entry in /build/build_tests.rs that describes the .poni file name
3. Create the expected test result
    - This may involve one of the following as-of-yet undecided mechanisms:
        1. Create a comment in the .poni file, such as // Expected: "result"
        2. Create a #directive or similar in the .poni file describing the result
        3. Simply add the information to build_tests.rs directly (this is likely the most straightforward, but makes the test scripts less directly useful).

Finally, in order to do error codes, the following mechanism is used:

1. Each error that we add to the compiler is given a UUID, created with `uuidgen -r`.
2. The test simply expects an error with a given UUID. (Perhaps as well with a location, but this is less certain as those may be unstable).

It should be the case that the UUIDs never collide, due to the relatively small number of errors in the compiler. Note importantly that the code for an error should generally not change. If it does, of course, the affected tests will have to be fixed.
