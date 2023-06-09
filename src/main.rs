
fn main() {
    let link = "magnet:?xt=urn:btih:73103935E5CA2B132DA9C5B716A012CEFC67E6BA&dn=Succession.S03E06.1080p.WEB.H264-CAKES&tr=http%3A%2F%2Ftracker.trackerfix.com%3A80%2Fannounce&tr=udp%3A%2F%2F9.rarbg.me%3A2800%2Fannounce&tr=udp%3A%2F%2F9.rarbg.to%3A2950%2Fannounce&tr=udp%3A%2F%2Ftracker.thinelephant.org%3A12740%2Fannounce&tr=udp%3A%2F%2Ftracker.fatkhoala.org%3A13720%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=http%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce&tr=udp%3A%2F%2Fopentracker.i2p.rocks%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.internetwarriors.net%3A1337%2Fannounce&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969%2Fannounce&tr=udp%3A%2F%2Fcoppersurfer.tk%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.zer0day.to%3A1337%2Fannounce";
    magdl::Magdl::download(String::from(link));
}
