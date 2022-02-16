use std::{env::args, fs::File, io::Read, process::exit};

use garble_script::{ast::ParamDef, check, eval::Computation, io::Literal};

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = args().collect();
    if args.len() < 2 {
        println!("usage: {} file [input1] [input2] ...", args[0]);
        exit(64);
    }
    let mut f = File::open(&args[1])?;
    let mut prg = String::new();
    f.read_to_string(&mut prg)?;

    let checked = check(&prg);
    match checked {
        Ok(checked) => {
            let main_params = &checked.main.params;
            if main_params.len() != args.len() - 2 {
                println!(
                    "Expected {} inputs, but found only {}: {:?}",
                    main_params.len(),
                    args.len() - 2,
                    &args[2..]
                );
            }
            let mut params = Vec::with_capacity(main_params.len());
            for ((party, ParamDef(_, ty)), arg) in main_params.iter().zip(&args[2..]) {
                let param = Literal::parse(&checked, ty, arg);
                match param {
                    Ok(param) => params.push((*party, param)),
                    Err(e) => {
                        println!("{}", e.prettify(&arg));
                        exit(65);
                    }
                }
            }
            let circuit = checked.compile();
            let mut computation = Computation::from(circuit);
            for (party, param) in params {
                computation.set_literal(&checked, party, param);
            }
            if let Err(e) = computation.run() {
                println!("{}", e.prettify(""));
                exit(65);
            };
            let ret_ty = &checked.main.body.1;
            let result = computation.get_literal(&checked, ret_ty);
            match result {
                Ok(result) => {
                    println!("{}", result);
                }
                Err(e) => {
                    println!("{}", e.prettify(""));
                    exit(70);
                }
            }
            Ok(())
        }
        Err(e) => {
            println!("{}", e.prettify(&prg));
            exit(65);
        }
    }
}