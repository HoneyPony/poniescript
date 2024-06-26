#include <stdio.h>

int main() {
	FILE *output = fopen("big2.poni", "w");
	if(!output) return 1;

	for(int i = 0; i < 1000000; ++i) {
		fprintf(output, "var a%d = 1 + 2 + 3 + 4;\n", i);
	}

	fclose(output);
}
