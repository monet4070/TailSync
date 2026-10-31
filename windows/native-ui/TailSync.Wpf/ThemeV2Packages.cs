using System.Text.Json;
using System.Text.RegularExpressions;

namespace TailSync.Wpf;

internal sealed record PendingThemePackage(string Path, string Digest, string Version, string? UpdateThemeId,
    string? VersionRelation, IReadOnlyList<JsonElement> Previews, IReadOnlyList<string> Diagnostics);

internal static class ThemeV2Packages
{
    internal static async Task<PendingThemePackage> ValidateAsync(string path, string? updateThemeId = null, string? installedVersion = null,
        CancellationToken token = default, Func<string, string, bool, CancellationToken, Task<JsonElement>>? validate = null)
    {
        validate ??= (file, mode, highContrast, cancellation) => NativeBackend.InvokeAsync<JsonElement>("validate_theme", new { path = file, mode, highContrast }, cancellation);
        var modes = new[] { ("light", false), ("dark", false), ("light", true), ("dark", true) };
        var validations = await Task.WhenAll(modes.Select(x => validate(path, x.Item1, x.Item2, token)));
        var previews = new List<JsonElement>(); var diagnostics = new List<string>();
        var digest = ThemeV2.Text(validations[0], "digest");
        var version = ThemeV2.Text(validations[0], "candidateVersion");
        foreach (var validation in validations)
        {
            if (ThemeV2.Value(validation, "diagnostics") is { ValueKind: JsonValueKind.Array } list)
                diagnostics.AddRange(list.EnumerateArray().Select(FormatDiagnostic));
            if (!ThemeV2.Bool(validation, "valid") || digest.Length == 0 || ThemeV2.Text(validation, "digest") != digest
                || version.Length == 0 || ThemeV2.Text(validation, "candidateVersion") != version
                || ThemeV2.Value(validation, "preview") is not { ValueKind: JsonValueKind.Object } preview)
                throw new InvalidOperationException(string.Join("\n", diagnostics.Distinct().DefaultIfEmpty("主题包的四种模式校验失败，或文件在校验过程中发生变化")));
            if (updateThemeId != null && ThemeV2.Text(preview, "id") != updateThemeId)
                throw new InvalidOperationException("更新包的主题 ID 与已安装主题不一致");
            previews.Add(preview.Clone());
        }
        var relation = updateThemeId != null ? CompareVersions(version, installedVersion ?? "") : null;
        return new(path, digest, version, updateThemeId, relation, previews, diagnostics.Distinct().ToArray());
    }

    internal static async Task<JsonElement> ApplyAsync(PendingThemePackage package, CancellationToken token = default)
    {
        if (package.UpdateThemeId == null)
            return await NativeBackend.InvokeAsync<JsonElement>("install_theme", new { path = package.Path, expectedDigest = package.Digest }, token);
        return await NativeBackend.InvokeAsync<JsonElement>("update_theme", new
        {
            path = package.Path, expectedDigest = package.Digest,
            options = new { allowSameVersion = package.VersionRelation == "same", allowDowngrade = package.VersionRelation == "downgrade" }
        }, token);
    }

    internal static string FormatDiagnostic(JsonElement diagnostic)
    {
        var location = ThemeV2.Text(diagnostic, "jsonPointer");
        var platforms = ThemeV2.Value(diagnostic, "platforms") is { ValueKind: JsonValueKind.Array } values ? string.Join(", ", values.EnumerateArray().Where(x => x.ValueKind == JsonValueKind.String).Select(x => x.GetString())) : "";
        var context = new[] { location, platforms, ThemeV2.Bool(diagnostic, "recoverable") ? "recoverable" : "not recoverable", ThemeV2.Bool(diagnostic, "fallbackApplied") ? "fallback applied" : "fallback not applied" };
        return $"[{ThemeV2.Text(diagnostic, "severity", "error")}] {ThemeV2.Text(diagnostic, "code")}: {ThemeV2.Text(diagnostic, "message")}" + " (" + string.Join("; ", context.Where(x => x.Length > 0)) + ")";
    }

    internal static string FormatError(Exception error)
    {
        if (error is BackendException backend && ThemeV2.Text(backend.Detail, "code").Length > 0) return FormatDiagnostic(backend.Detail);
        try
        {
            using var json = JsonDocument.Parse(error.Message);
            if (ThemeV2.Text(json.RootElement, "code").Length > 0) return FormatDiagnostic(json.RootElement);
        }
        catch (JsonException) { }
        return error.Message;
    }

    internal static string CompareVersions(string candidate, string installed)
    {
        var pattern = @"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$";
        var a = Regex.Match(candidate, pattern); var b = Regex.Match(installed, pattern);
        bool Valid(Match value) => value.Success && (!value.Groups[4].Success || value.Groups[4].Value.Split('.').All(x => !x.All(char.IsDigit) || x.Length == 1 || x[0] != '0'));
        if (!Valid(a) || !Valid(b)) throw new FormatException("主题版本必须是有效的 SemVer");
        int Numeric(string x, string y) => x.Length != y.Length ? x.Length.CompareTo(y.Length) : string.CompareOrdinal(x, y);
        string Relation(int comparison) => comparison > 0 ? "upgrade" : comparison < 0 ? "downgrade" : "same";
        for (var index = 1; index <= 3; index++)
        {
            var comparison = Numeric(a.Groups[index].Value, b.Groups[index].Value);
            if (comparison != 0) return Relation(comparison);
        }
        if (!a.Groups[4].Success && !b.Groups[4].Success) return "same";
        if (!a.Groups[4].Success) return "upgrade";
        if (!b.Groups[4].Success) return "downgrade";
        var left = a.Groups[4].Value.Split('.'); var right = b.Groups[4].Value.Split('.');
        for (var index = 0; index < Math.Max(left.Length, right.Length); index++)
        {
            if (index == left.Length) return "downgrade";
            if (index == right.Length) return "upgrade";
            var x = left[index]; var y = right[index];
            if (x == y) continue;
            var xNumeric = x.All(char.IsDigit); var yNumeric = y.All(char.IsDigit);
            if (xNumeric && yNumeric) return Relation(Numeric(x, y));
            if (xNumeric != yNumeric) return xNumeric ? "downgrade" : "upgrade";
            return Relation(string.CompareOrdinal(x, y));
        }
        return "same";
    }
}
