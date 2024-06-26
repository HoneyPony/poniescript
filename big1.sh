echo > big1.poni
for i in $(seq 1 100000)
do
	echo "var a$i = 1 + 2 + 3 + 4;" >> big1.poni
done
