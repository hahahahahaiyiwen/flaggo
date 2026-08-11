namespace Flaggo.DataPlane;

public static class LocalTargetingHosting
{
    public static IReadOnlyDictionary<string, string>
        CreateAuthoritativeCohorts(IConfiguration configuration)
    {
        var mappings = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["new_players"] = "new_players",
            ["whales"] = "new_players"
        };

        foreach (var mapping in configuration
                     .GetSection("Flaggo:Targeting:AuthoritativeCohorts")
                     .GetChildren())
        {
            if (string.IsNullOrWhiteSpace(mapping.Key) ||
                string.IsNullOrWhiteSpace(mapping.Value))
            {
                throw new InvalidOperationException(
                    "Authoritative cohort mappings require non-empty claimed and resolved cohort identifiers.");
            }

            mappings[mapping.Key] = mapping.Value;
        }

        return mappings;
    }
}
