public class bench {

    static int fib(int n) {
        if (n < 2) return n;
        return fib(n - 1) + fib(n - 2);
    }

    static void benchFibonacci() {
        long start = System.currentTimeMillis();
        int result = fib(40);
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("fibonacci:" + elapsed);
        System.out.println("  checksum: " + result);
    }

    static void benchLoopOverhead() {
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 100_000_000; i++) {
            sum += 1.0;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("loop_overhead:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchArrayWrite() {
        double[] arr = new double[10_000_000];
        long start = System.currentTimeMillis();
        for (int i = 0; i < 10_000_000; i++) {
            arr[i] = (double) i;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("array_write:" + elapsed);
        System.out.printf("  checksum: %.0f%n", arr[9_999_999]);
    }

    static void benchArrayRead() {
        double[] arr = new double[10_000_000];
        for (int i = 0; i < 10_000_000; i++) {
            arr[i] = (double) i;
        }
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 10_000_000; i++) {
            sum += arr[i];
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("array_read:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchMathIntensive() {
        long start = System.currentTimeMillis();
        double result = 0.0;
        for (int i = 1; i <= 50_000_000; i++) {
            result += 1.0 / (double) i;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("math_intensive:" + elapsed);
        System.out.printf("  checksum: %.6f%n", result);
    }

    static class Point {
        double x;
        double y;

        Point(double x, double y) {
            this.x = x;
            this.y = y;
        }
    }

    static void benchObjectCreate() {
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 1_000_000; i++) {
            Point p = new Point((double) i, (double) i * 2.0);
            sum += p.x + p.y;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("object_create:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchNestedLoops() {
        int n = 3000;
        double[] arr = new double[n * n];
        for (int i = 0; i < n * n; i++) {
            arr[i] = (double) i;
        }
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                sum += arr[i * n + j];
            }
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("nested_loops:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchAccumulate() {
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 100_000_000; i++) {
            sum += (double) (i % 1000);
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("accumulate:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    public static void main(String[] args) {
        benchFibonacci();
        benchLoopOverhead();
        benchArrayWrite();
        benchArrayRead();
        benchMathIntensive();
        benchObjectCreate();
        benchNestedLoops();
        benchAccumulate();
    }
}
