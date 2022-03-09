use std::{env::args, fs::File, io::Read, process::exit};

use garble::{ast::ParamDef, check, eval::Evaluator, literal::Literal};

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} file [input1] [input2] ...", args[0]);
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
                eprintln!(
                    "Expected {} inputs, but found {}: {:?}",
                    main_params.len(),
                    args.len() - 2,
                    &args[2..]
                );
                exit(65);
            }
            let mut params = Vec::with_capacity(main_params.len());
            for (i, (ParamDef(_, ty), arg)) in main_params.iter().zip(&args[2..]).enumerate() {
                let param = Literal::parse(&checked, ty, arg);
                match param {
                    Ok(param) => params.push(param),
                    Err(e) => {
                        eprintln!("Could not parse argument {i}!\n{}", e.prettify(arg));
                        exit(65);
                    }
                }
            }
            let circuit = checked.compile();
            let mut computation = Evaluator::from(&circuit);
            for param in params {
                if let Err(e) = computation.set_literal(&checked, param) {
                    eprintln!("{}", e.prettify(&prg));
                    exit(65);
                }
            }
            match computation.run() {
                Err(e) => {
                    eprintln!("{}", e.prettify(&prg));
                    exit(65);
                }
                Ok(output) => {
                    let result = output.into_literal(&checked);
                    match result {
                        Ok(result) => {
                            println!("{}", result);
                        }
                        Err(e) => {
                            eprintln!("{}", e.prettify(&prg));
                            exit(70);
                        }
                    }
                    Ok(())
                }
            }
        }
        Err(e) => {
            eprintln!("{}", e.prettify(&prg));
            exit(65);
        }
    }
}
