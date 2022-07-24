int complex_function(int a, int b) {
    return 2*a + b;
}

void segfault_here(int *x) {
    int *address = x + 10;
    int value = *address;
}

int main(int argc, char** argv) {
    int a = 1;
    int b = 2;
    int result = complex_function(a, b);
    segfault_here((int*)result);
    return 0;
}