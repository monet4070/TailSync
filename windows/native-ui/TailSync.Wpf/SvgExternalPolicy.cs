using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Text.RegularExpressions;
using System.Xml;
using System.Xml.Linq;
using SkiaSharp;

namespace TailSync.Wpf;

internal sealed record SvgExternalSummary(IReadOnlyList<Uri> Images, IReadOnlyList<string> AllowedOrigins, IReadOnlyList<string> RejectedTargets)
{
    internal bool CanGrant => Images.Count > 0 && Images.Count <= 16 && AllowedOrigins.Count > 0 && RejectedTargets.Count == 0;
}

// No network is used until the per-entry origin dialog is explicitly accepted.
// Authorized images are fetched into bounded in-memory PNGs, so the renderer
// itself never receives remote URLs, credentials, redirects, or local paths.
internal static class SvgExternalPolicy
{
    internal const int MaximumImageBytes=4*1024*1024;
    internal static SvgExternalSummary Summarize(ReadOnlyMemory<byte> bytes)
    {
        var document=Parse(bytes);var images=new List<Uri>();var origins=new List<string>();var rejected=new List<string>();
        if(document.Descendants().Any(node=>node.Name.LocalName is "script" or "foreignObject" or "iframe" or "object" or "embed"))rejected.Add("Active SVG markup");
        foreach(var node in document.Descendants())
        {
            foreach(var attribute in node.Attributes().Where(value=>value.Name.LocalName is "href" or "src"))
            {
                var target=attribute.Value.Trim();
                if(target.Length==0||target.StartsWith('#')||target.StartsWith("data:",StringComparison.OrdinalIgnoreCase))continue;
                if(!Uri.TryCreate(target,UriKind.Absolute,out var uri)||!Eligible(uri)) {rejected.Add(target);continue;}
                // Only explicit raster image nodes are eligible for network access.
                // External use/style/font references remain blocked and disclosed.
                if(node.Name.LocalName!="image"){rejected.Add(target);continue;}
                if(!images.Contains(uri))images.Add(uri);
                var origin=uri.GetLeftPart(UriPartial.Authority);if(!origins.Contains(origin,StringComparer.OrdinalIgnoreCase))origins.Add(origin);
            }
            foreach(var attribute in node.Attributes().Where(value=>value.Name.LocalName is "style" or "fill" or "filter" or "stroke"))
                RejectCssReferences(attribute.Value,rejected);
            if(node.Name.LocalName=="style")RejectCssReferences(node.Value,rejected);
        }
        if(images.Count>16)rejected.Add("SVG contains more than 16 external images");
        return new(images,origins,rejected.Distinct().ToArray());
    }
    private static void RejectCssReferences(string value,List<string> rejected)
    {
        foreach(Match match in Regex.Matches(value,@"url\s*\(\s*['"" ]?([^)'""\s]+)",RegexOptions.IgnoreCase))
            if(!match.Groups[1].Value.StartsWith('#'))rejected.Add(match.Groups[1].Value);
        if(value.Contains("@import",StringComparison.OrdinalIgnoreCase))rejected.Add("CSS @import");
    }
    internal static bool Eligible(Uri uri)
    {
        if(uri.Scheme!=Uri.UriSchemeHttps||uri.UserInfo.Length>0||uri.Host.Length==0||uri.Port is <=0 or >65535)return false;
        var host=uri.IdnHost.TrimEnd('.').ToLowerInvariant();
        if(host=="localhost"||host.EndsWith(".localhost")||host.EndsWith(".local")||!host.Contains('.')&&!IPAddress.TryParse(host,out _))return false;
        return !IPAddress.TryParse(host,out var address)||PublicAddress(address);
    }
    internal static bool PublicAddress(IPAddress address)
    {
        if(IPAddress.IsLoopback(address))return false;
        if(address.IsIPv4MappedToIPv6)return PublicAddress(address.MapToIPv4());
        var b=address.GetAddressBytes();
        if(b.Length==4)
        {
            if(b[0] is 0 or 10 or 127||b[0]>=224||b[0]==169&&b[1]==254||b[0]==172&&b[1] is >=16 and <=31||b[0]==192&&b[1]==168)return false;
            if(b[0]==100&&b[1] is >=64 and <=127||b[0]==198&&b[1] is 18 or 19)return false;
            if(b[0]==192&&b[1]==0||b[0]==192&&b[1]==2||b[0]==198&&b[1]==51&&b[2]==100||b[0]==203&&b[1]==0&&b[2]==113)return false;
            return true;
        }
        // Accept ordinary global unicast only. Reject local, mapped, multicast,
        // documentation, transition tunnels and special-purpose 2001::/23.
        return b.Length==16&&(b[0]&0xe0)==0x20&&!(b[0]==0x20&&b[1]==1&&b[2]<2)&&!(b[0]==0x20&&b[1]==1&&b[2]==0x0d&&b[3]==0xb8)&&!(b[0]==0x20&&b[1]==2);
    }
    internal static async Task<byte[]> MaterializeAsync(ReadOnlyMemory<byte> original,SvgExternalSummary grant,CancellationToken cancellation)
    {
        if(!grant.CanGrant)throw new InvalidOperationException("SVG external image sources are not eligible for authorization");
        using var timeout=CancellationTokenSource.CreateLinkedTokenSource(cancellation);timeout.CancelAfter(TimeSpan.FromSeconds(12));var token=timeout.Token;
        using var handler=new SocketsHttpHandler
        {
            AllowAutoRedirect=false,UseProxy=false,UseCookies=false,Credentials=null,ConnectTimeout=TimeSpan.FromSeconds(4),
            ConnectCallback=async(context,connectToken)=>
            {
                var addresses=await Dns.GetHostAddressesAsync(context.DnsEndPoint.Host,connectToken);
                if(addresses.Length==0||addresses.Any(address=>!PublicAddress(address)))throw new HttpRequestException("SVG image origin resolves to a non-public address");
                Exception? last=null;
                foreach(var address in addresses)
                {
                    var socket=new Socket(address.AddressFamily,SocketType.Stream,ProtocolType.Tcp);
                    try{await socket.ConnectAsync(new IPEndPoint(address,context.DnsEndPoint.Port),connectToken);return new NetworkStream(socket,true);}
                    catch(Exception error){socket.Dispose();last=error;}
                }
                throw new HttpRequestException("SVG image origin could not be connected",last);
            }
        };
        using var client=new HttpClient(handler){Timeout=Timeout.InfiniteTimeSpan};
        var replacements=new Dictionary<string,string>(StringComparer.Ordinal);long totalBytes=0,normalizedBytes=0;
        foreach(var uri in grant.Images)
        {
            if(!Eligible(uri)||!grant.AllowedOrigins.Contains(uri.GetLeftPart(UriPartial.Authority),StringComparer.OrdinalIgnoreCase))throw new HttpRequestException("SVG source differs from its explicit origin grant");
            using var request=new HttpRequestMessage(HttpMethod.Get,uri);request.Headers.Accept.ParseAdd("image/png,image/jpeg,image/webp,image/gif");
            using var response=await client.SendAsync(request,HttpCompletionOption.ResponseHeadersRead,token);
            if((int)response.StatusCode is >=300 and <400)throw new HttpRequestException("SVG image redirects are blocked");response.EnsureSuccessStatusCode();
            if(response.Content.Headers.ContentLength>MaximumImageBytes)throw new InvalidDataException("SVG image exceeds 4 MB");
            var mime=response.Content.Headers.ContentType?.MediaType;
            if(mime is not ("image/png" or "image/jpeg" or "image/webp" or "image/gif"))throw new InvalidDataException("SVG external content is not a supported raster image");
            await using var input=await response.Content.ReadAsStreamAsync(token);using var output=new MemoryStream();var buffer=new byte[16384];
            while(true){var read=await input.ReadAsync(buffer,token);if(read==0)break;if(output.Length+read>MaximumImageBytes||totalBytes+read>16*1024*1024)throw new InvalidDataException("SVG external image budget exceeded");output.Write(buffer,0,read);totalBytes+=read;}
            var compressed=output.ToArray();
            var png=await Task.Run(()=>NormalizeRaster(compressed),token);
            normalizedBytes+=png.Length;if(normalizedBytes>16*1024*1024)throw new InvalidDataException("SVG normalized image budget exceeded");
            replacements[uri.AbsoluteUri]="data:image/png;base64,"+Convert.ToBase64String(png);
        }
        var document=Parse(original);
        foreach(var attribute in document.Descendants().Where(node=>node.Name.LocalName=="image").SelectMany(node=>node.Attributes()).Where(value=>value.Name.LocalName is "href" or "src"))
            if(Uri.TryCreate(attribute.Value,UriKind.Absolute,out var uri)&&replacements.TryGetValue(uri.AbsoluteUri,out var embedded))attribute.Value=embedded;
        return Encoding.UTF8.GetBytes(document.ToString(SaveOptions.DisableFormatting));
    }
    internal static byte[] NormalizeRaster(byte[] compressed)
    {
        using var data=SKData.CreateCopy(compressed);using var codec=SKCodec.Create(data)??throw new InvalidDataException("SVG external image is corrupt");var info=codec.Info;
        if(info.Width<=0||info.Height<=0||info.Width>4096||info.Height>4096||(long)info.Width*info.Height>16*1024*1024)throw new InvalidDataException("SVG external image dimensions exceed the preview budget");
        using var bitmap=SKBitmap.Decode(codec)??throw new InvalidDataException("SVG external image could not be decoded");using var encoded=bitmap.Encode(SKEncodedImageFormat.Png,100);
        if(encoded.Size>MaximumImageBytes)throw new InvalidDataException("SVG normalized image exceeds 4 MB");return encoded.ToArray();
    }
    private static XDocument Parse(ReadOnlyMemory<byte> bytes)
    {
        if(bytes.Length>8*1024*1024)throw new InvalidDataException("SVG exceeds the safe visual preview limit");
        using var stream=PreviewRenderers.Stream(bytes);using var reader=XmlReader.Create(stream,new XmlReaderSettings{DtdProcessing=DtdProcessing.Prohibit,XmlResolver=null,MaxCharactersInDocument=32*1024*1024});return XDocument.Load(reader);
    }
}
