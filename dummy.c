#include <stdio.h>
#include <signal.h>

int main(int argc, char** argv) {
    // FILE *fp;
    // fp = fopen("myfile.txt", "w+");
    // fprintf(fp, "Main is at 0x%x\n", (unsigned)main);
    int a = 1 + 3;
    // raise(SIGTRAP);
    fprintf(stdout, "Main is at 0x%lx\n", (size_t)main);
    //fclose(fp);
}