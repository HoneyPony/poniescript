#include <stdio.h>

int main() {
	FILE *output = fopen("big4.poni", "w");
	if(!output) return 1;

	for(int i = 0; i < 1000000; i += 5) {
		fprintf(output, "fun a%d() {\n", i);
		fprintf(output, "\tvar a = (1, 2, 3);\n");
		fprintf(output, "\tvar b = (4, 5, 6);\n");
		float t = i / 1000000.0;
		fprintf(output, "\tprint(lerp(a, b, %f));\n", t);
		fprintf(output, "}\n");
	}

	fclose(output);
}
