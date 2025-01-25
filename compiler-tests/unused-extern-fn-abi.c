

__attribute__((noinline))
void
public_noattr(void *firstarg, int a, int b);

__attribute__((noinline))
void
public_attr(__attribute__((unused)) void *firstarg, int a, int b);

int
main() {
    public_noattr((void*)1, 10, 20);
    public_attr((void*)2, 10, 20);
}