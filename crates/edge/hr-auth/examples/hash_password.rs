fn main() {
    let password = std::env::args().nth(1).expect("Usage: hash_password <pw>");
    let hash = hr_auth::users::hash_password(&password).expect("Hash failed");
    println!("{}", hash);
}
