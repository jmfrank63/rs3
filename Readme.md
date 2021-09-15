# Rust based information object store
## MSc Computer Science Project
## jfrank11 DCS Birkbeck

###Running the server

Clone the project  
`git clone https://github.com/Birkbeck/msc-computer-science-project-2020_21---files-jmfrank63.git`  
If not installed, install rust  
Create a .env file. Use `env.example` as a template  
`cd` into the project directory
`cargo run`  
You should see something like  
`Starting Http server at host address: localhost, with port: 5000!`  
on the screen.  
The server is now ready to server calls.  
`curl https://localhost:5000/`
### Running the tests
`cargo test`




