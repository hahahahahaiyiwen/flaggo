namespace Flaggo.Policy;

internal static class NumericPolicy
{
    public static bool IsOnGrid(double value, double minimum, double step)
    {
        var quotient = (value - minimum) / step;
        return double.IsFinite(quotient) &&
            Math.Abs(quotient - Math.Round(quotient)) <= 1e-9;
    }
}
