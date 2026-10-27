//! A model that cannot read images, and a place list that is blind to the screen.

use crate::scripted::*;
use cua_run::{CuaModel, CuaModelError, ModelSight};
use cua_system::InferdCuaModel;
use porter_core::{AccountId, ModelId};
use porter_infer::{
    Declined, DeclinedBecause, InferEvent, InferRefusal, InferReply, ModelRef, PlaceId, PlaceModel,
    PlaceRow, PlaceState, Takes,
};

fn declined_blind() -> InferEvent {
    InferEvent::Declined(Declined {
        model: ModelRef {
            account: AccountId::parse("local").expect("account"),
            model: ModelId::parse("qwen-text").expect("model"),
        },
        because: DeclinedBecause::CannotSeeScreen,
    })
}

#[tokio::test]
async fn a_chosen_model_that_cannot_read_images_is_told_by_name_and_not_retried() {
    let (transport, _) = Scripted::new(vec![vec![
        declined_blind(),
        InferEvent::Finished(InferReply::Refused(InferRefusal::Unavailable)),
    ]]);
    let mut model = InferdCuaModel::over(transport);
    assert_eq!(
        model.begin(begin()).await,
        Err(CuaModelError::CannotSeeScreen)
    );
    assert_eq!(
        model.cannot_see_words(),
        Some(porter_infer::cannot_see_screen("qwen-text"))
    );
}

#[tokio::test]
async fn no_model_that_reads_images_says_porters_sentence() {
    let (transport, _) = Scripted::new(vec![vec![InferEvent::Finished(InferReply::Refused(
        InferRefusal::NoModelSeesScreen,
    ))]]);
    let mut model = InferdCuaModel::over(transport);
    assert_eq!(
        model.begin(begin()).await,
        Err(CuaModelError::CannotSeeScreen)
    );
    assert_eq!(
        model.cannot_see_words().as_deref(),
        Some(porter_infer::NO_MODEL_SEES_SCREEN)
    );
}

fn place(models: &[Takes], state: PlaceState) -> PlaceRow {
    let models = models
        .iter()
        .enumerate()
        .map(|(n, takes)| {
            PlaceModel::new(
                ModelId::parse(&format!("m{n}")).expect("model"),
                format!("M{n}"),
            )
            .with_takes(*takes)
        })
        .collect();
    PlaceRow::new(
        PlaceId::this_computer(),
        porter_infer::PlaceKind::ThisComputer,
        "This computer".into(),
        state,
    )
    .with_models(models)
}

#[tokio::test]
async fn only_models_all_known_not_to_read_images_make_the_screen_blind() {
    let ready = PlaceState::Ready;
    let rows = [
        (
            vec![place(&[Takes::TextOnly, Takes::TextAndAudio], ready)],
            ModelSight::Blind,
        ),
        (
            vec![place(&[Takes::TextOnly, Takes::TextAndImages], ready)],
            ModelSight::MaySee,
        ),
        (
            vec![place(&[Takes::TextOnly, Takes::Unknown], ready)],
            ModelSight::MaySee,
        ),
        (
            vec![place(&[Takes::TextOnly], PlaceState::NotReady)],
            ModelSight::MaySee,
        ),
        (vec![], ModelSight::MaySee),
    ];
    for (places, want) in rows {
        let mut model = InferdCuaModel::over(Scripted::placing(places.clone()));
        assert_eq!(model.sight().await, want, "{places:?}");
    }
    let mut silent = InferdCuaModel::over(Scripted::unreachable());
    assert_eq!(
        silent.sight().await,
        ModelSight::MaySee,
        "no answer is no warning"
    );
}
