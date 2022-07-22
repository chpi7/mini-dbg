#include <stdio.h>

int main(int argc, char** argv) {
    FILE *fp;

    fp = fopen("myfile.txt", "w+");
    // fprintf(fp, "Main is at 0x%x\n", (unsigned)main);
    fprintf(stdout, "Main is at 0x%x\n", (unsigned)main);
    fclose(fp);
}