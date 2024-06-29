#include <stdio.h>

int main() {
	FILE *output = fopen("big3.poni", "w");
	if(!output) return 1;

	for(int i = 0; i < 1000000; i += 3) {
		fprintf(output, "fun a%d() {\n\tvar x = 1 + 2 + 3 + 4;\n}\n", i);
	}

	fclose(output);
}
