int complex_function(int a, int b) {
    return 2*a + b;
}

int segfault_here(int *x) {
    int *address = x + 10;
    int value = *address;
    return value;
}

int main(int argc, char** argv) {
    int a = 1;
    int b = 2;
    int result = complex_function(a, b);
    int other_result = segfault_here((int*)result);
    return 0;
}