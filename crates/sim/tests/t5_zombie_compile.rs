//! Offline compile of T5's zombie scripts against the T5 catalog. Needs an
//! extracted corpus: `T5_GSC_DIR` points at a directory of `<module>.gsc`.

use sim::script::{Catalog, FileSources, Program};

#[test]
#[ignore = "needs an extracted T5 corpus in T5_GSC_DIR"]
fn zombie_theater_compiles_against_t5_catalog() {
    let dir = std::env::var("T5_GSC_DIR").expect("T5_GSC_DIR");
    let roots_env = std::env::var("T5_GSC_ROOTS")
        .unwrap_or_else(|_| "maps/_zombiemode,maps/zombie_theater".to_owned());
    let roots: Vec<&str> = roots_env.split(',').collect();
    // BO1 zombies runs on the single-player build's builtins.
    let result = Program::load(&FileSources(dir.into()), &roots, &Catalog::t5_sp());
    match result {
        Ok(program) => println!(
            "compiled: modules={} functions={} natives={}",
            program.modules().len(),
            program.function_count(),
            program.native_count()
        ),
        Err(fault) => panic!("fault: {fault}\n{:#?}", fault.callers),
    }
}
