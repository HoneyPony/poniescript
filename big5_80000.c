#include <stdio.h>

int main() {
	FILE *output = fopen("big5_80000.poni", "w");
	if(!output) return 1;

	fprintf(output, "class Horse {\n");
	fprintf(output, "\tvar name: StrBuf = \"twilight\";\n");
	fprintf(output, "\tvar style: int = 5;\n");
	fprintf(output, "}\n");

	for(int i = 0; i < 80000 - 4; i += 29) {
		fprintf(output, "fun a%d() {\n", i);
		fprintf(output, "\tvar a = (1, 2, 3);\n");
		fprintf(output, "\tvar b = (4, 5, 6);\n");
		float t = i / 80000.0;
		fprintf(output, "\tprint(lerp(a, b, %f));\n", t);
		fprintf(output, "\tvar arr = [1, 2, 3, 4];\n");
		fprintf(output, "\tprint(arr[0], arr[1], arr[2], arr[3]);\n");
		fprintf(output, "\tif(arr[2] > a .2) {\n");
		fprintf(output, "\t\tprint(\"hello\");\n");
		fprintf(output, "\t}\n");
		fprintf(output, "\telse {\n");
		fprintf(output, "\t\tprint(\"goodbye\");\n");
		fprintf(output, "\t}\n");
		fprintf(output, "\tvar horse = new Horse { style: %d };\n", i);
		fprintf(output, "\tprint(horse.name, \" \", horse.style);\n");
		fprintf(output, "\thorse.name = str(\"my\", \" \", \"little\", \" \", \"pony\");\n");
		fprintf(output, "\tprint(horse.name, \" \", horse.style);\n");
		fprintf(output, "\twhile horse.style < 40 {\n");
		fprintf(output, "\t\tprint(horse.style);\n");
		fprintf(output, "\t}\n");
		fprintf(output, "\tvar evaluation = loop {\n");
		fprintf(output, "\t\tif horse.style > 42 {\n");
		fprintf(output, "\t\t\tbreak \"yes\";\n");
		fprintf(output, "\t\t}\n");
		fprintf(output, "\t\telse {\n");
		fprintf(output, "\t\t\tbreak \"no\";\n");
		fprintf(output, "\t\t}\n");
		fprintf(output, "\t};\n");
		fprintf(output, "\tprint(evaluation);\n");
		fprintf(output, "}\n");
	}

	fclose(output);
}
